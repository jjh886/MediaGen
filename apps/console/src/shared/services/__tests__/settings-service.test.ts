import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  clearCache,
  getAllSettings,
  getCacheSize,
  getSetting,
  setSetting,
} from '../settings-service'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

const fetchMock = vi.fn()

describe('settings-service', () => {
  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('服务端没有返回设置时抛出错误，避免界面展示假默认值', async () => {
    fetchMock.mockRejectedValue(new Error('server offline'))

    await expect(getSetting('api_port')).rejects.toThrow('server offline')
  })

  it('服务端设置优先级高于默认值', async () => {
    fetchMock.mockResolvedValue(jsonResponse({ data: { api_port: '18081' } }))

    expect(await getSetting('api_port')).toBe(18081)
  })

  it('字符串配置里的 JSON 原文不会被误转成对象', async () => {
    const overrides = '{"codex_free":{"concurrency_limit":1}}'
    fetchMock.mockResolvedValue(jsonResponse({ data: { platform_tier_overrides: overrides } }))

    expect(await getSetting('platform_tier_overrides')).toBe(overrides)
  })

  it('setSetting 保存普通设置时同步写入服务端', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: { cache_dir: '/tmp/mediagen' } }))

    await setSetting('cache_dir', '/tmp/mediagen')

    expect(fetchMock).toHaveBeenCalledWith('/api/settings', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ cache_dir: '/tmp/mediagen' }),
    })
  })

  it('getAllSettings 合并服务端返回和默认值', async () => {
    fetchMock.mockResolvedValue(jsonResponse({ data: { api_port: '18082' } }))

    const settings = await getAllSettings()

    expect(settings.api_port).toBe(18082)
    expect(settings.api_address).toBe('0.0.0.0')
    expect(settings.image_task_concurrency).toBe(64)
  })

  it('忽略旧版多端口字段，保留服务端实际监听配置', async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({
        data: {
          api_port: 19080,
          api_address: '192.0.2.10',
          api_end_port: 19089,
          api_ports: '19080-19089',
          api_port_count: 10,
        },
      })
    )

    const settings = await getAllSettings()

    expect(settings.api_port).toBe(19080)
    expect(settings.api_address).toBe('192.0.2.10')
    expect(settings).not.toHaveProperty('api_end_port')
    expect(settings).not.toHaveProperty('api_ports')
    expect(settings).not.toHaveProperty('api_port_count')
  })

  it('缓存清理暂未接入真实文件清理', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ ok: true }))

    await expect(getCacheSize()).resolves.toBe('暂未接入真实清理')
    await expect(clearCache()).resolves.toBeUndefined()
    expect(fetchMock).toHaveBeenCalledWith('/api/cache/clear', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: undefined,
    })
  })
})

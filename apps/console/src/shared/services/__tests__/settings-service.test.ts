import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  checkBrowserRuntimeAvailable,
  checkBrowserRuntimeVersion,
  clearCache,
  getAllSettings,
  getBrowserRuntimeInstallGuide,
  getCacheSize,
  getSetting,
  installBrowserRuntime,
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

  it('服务端没有返回设置时使用默认值', async () => {
    fetchMock.mockRejectedValue(new Error('server offline'))

    expect(await getSetting('api_port')).toBe(18080)
    expect(await getSetting('api_address')).toBe('127.0.0.1')
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

  it('setSetting 保存用户 API Key 时同步写入服务端', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: { api_keys: 'key-a\\nkey-b' } }))

    await setSetting('api_keys', 'key-a\nkey-b')

    expect(fetchMock).toHaveBeenCalledWith('/api/settings', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ api_keys: 'key-a\nkey-b' }),
    })
  })

  it('getAllSettings 合并服务端返回和默认值', async () => {
    fetchMock.mockResolvedValue(jsonResponse({ data: { api_port: '18082' } }))

    const settings = await getAllSettings()

    expect(settings.api_port).toBe(18082)
    expect(settings.api_address).toBe('127.0.0.1')
    expect(settings.image_task_concurrency).toBe(64)
  })

  it('检测到健康 CloakBrowser 时返回版本信息', async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        data: [
          {
            id: 'cloakbrowser',
            name: 'CloakBrowser',
            installed: true,
            path: '/Applications/CloakBrowser.app',
            version: '130.0',
            healthy: true,
          },
        ],
      })
    )

    await expect(checkBrowserRuntimeAvailable()).resolves.toEqual({
      ok: true,
      message: 'CloakBrowser 130.0',
    })
  })

  it('未检测到 CloakBrowser 时返回引导提示', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: [] }))

    const result = await checkBrowserRuntimeAvailable()

    expect(result.ok).toBe(false)
    expect(result.message).toContain('未检测到 CloakBrowser')
  })

  it('查询浏览器运行时版本', async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        data: [
          {
            id: 'cloakbrowser',
            name: 'CloakBrowser',
            installed: true,
            path: '/opt/cloakbrowser',
            version: '131.0',
            healthy: true,
          },
        ],
      })
    )

    await expect(checkBrowserRuntimeVersion()).resolves.toBe('131.0')
  })

  it('安装引导和自动安装都调用服务端运行时接口', async () => {
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ runtime: 'CloakBrowser', status: 'missing', steps: [], notes: [] })
      )
      .mockResolvedValueOnce(
        jsonResponse({
          data: {
            id: 'cloakbrowser',
            name: 'CloakBrowser',
            installed: true,
            path: '/cache/CloakBrowser',
            version: '132.0',
            healthy: true,
          },
        })
      )

    await expect(getBrowserRuntimeInstallGuide()).resolves.toMatchObject({
      runtime: 'CloakBrowser',
    })
    await expect(installBrowserRuntime()).resolves.toMatchObject({
      id: 'cloakbrowser',
      version: '132.0',
    })
  })

  it('缓存管理由服务端统一处理', async () => {
    await expect(getCacheSize()).resolves.toBe('由服务端统一管理')
    await expect(clearCache()).resolves.toBeUndefined()
  })
})

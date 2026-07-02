import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { exitMediaGen, getSystemInfo } from '../system-service'

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('system-service', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('读取服务端系统信息决定是否显示退出入口', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: { os: 'macos', exit_supported: true } }))

    await expect(getSystemInfo()).resolves.toEqual({ os: 'macos', exit_supported: true })
    expect(fetchMock).toHaveBeenCalledWith('/api/system/info', {
      headers: { Accept: 'application/json' },
    })
  })

  it('退出 MediaGen 时调用本机退出接口', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ ok: true }))

    await exitMediaGen()

    expect(fetchMock).toHaveBeenCalledWith('/api/system/exit', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
    })
  })
})

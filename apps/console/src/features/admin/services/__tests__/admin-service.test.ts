import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  createApiKey,
  createProxy,
  deleteApiKey,
  deleteProxy,
  fetchApiKeys,
  fetchBrowserSessions,
  fetchProxies,
  testProxy,
} from '../admin-service'

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('admin-service', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('读取管理列表接口', async () => {
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ data: [{ id: 'key-1', name: '用户A' }] }))
      .mockResolvedValueOnce(jsonResponse({ data: [{ id: 'proxy-1', server: 'socks5://p:1080' }] }))
      .mockResolvedValueOnce(
        jsonResponse({ data: [{ id: 'session-1', purpose: 'debug_preview' }] })
      )

    await expect(fetchApiKeys()).resolves.toHaveLength(1)
    await expect(fetchProxies()).resolves.toHaveLength(1)
    await expect(fetchBrowserSessions()).resolves.toHaveLength(1)

    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      '/api/api-keys?limit=40&offset=0',
      '/api/proxies?limit=40&offset=0',
      '/api/browser-sessions?limit=40&offset=0&takeover_only=true',
    ])
  })

  it('写入和删除管理实体时调用对应接口', async () => {
    fetchMock.mockImplementation(() => Promise.resolve(jsonResponse({ ok: true })))

    await createApiKey('用户A')
    await deleteApiKey('key-1')
    await createProxy({
      name: '住宅代理',
      server: 'socks5://127.0.0.1:1080',
      username: 'u',
      password: 'p',
    })
    await deleteProxy('proxy-1')
    await testProxy('proxy-1')

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/api-keys',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ name: '用户A' }),
      })
    )
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/api-keys/key-1',
      expect.objectContaining({ method: 'DELETE' })
    )
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/proxies',
      expect.objectContaining({ method: 'POST' })
    )
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/proxies/proxy-1',
      expect.objectContaining({ method: 'DELETE' })
    )
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/proxies/proxy-1/test',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({}),
      })
    )
  })
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { HttpApiError, apiGet, apiPost, apiDelete } from '../http-api-client'

describe('http-api-client', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('正常解包服务端 data 字段', async () => {
    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify({ data: { ok: true } }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    )

    await expect(apiGet('/health')).resolves.toEqual({ ok: true })
  })

  it('后端返回纯文本 404 时不会再抛 JSON 解析错误', async () => {
    fetchMock.mockResolvedValueOnce(
      new Response('Request did not match any route', { status: 404 })
    )

    await expect(apiGet('/api/missing')).rejects.toMatchObject({
      status: 404,
      message: 'Request did not match any route',
    } satisfies Partial<HttpApiError>)
  })
  it.each([
    ['读取', apiGet],
    ['保存', apiPost],
    ['删除', apiDelete],
  ])('%s管理接口会话失效时通知登录界面，认证接口错误不重复触发', async (_name, request) => {
    const expired = vi.fn()
    window.addEventListener('mediagen:auth-expired', expired)
    try {
      for (const path of ['/api/admin-users', '/api/auth/login']) {
        fetchMock.mockResolvedValueOnce(
          new Response(JSON.stringify({ error: '请重新登录' }), { status: 401 })
        )
        await expect(request(path)).rejects.toMatchObject({ status: 401, message: '请重新登录' })
      }
      expect(expired).toHaveBeenCalledTimes(1)
    } finally {
      window.removeEventListener('mediagen:auth-expired', expired)
    }
  })

  it('无错误正文时保留状态码，空成功响应正常解析', async () => {
    fetchMock.mockResolvedValueOnce(new Response('', { status: 503 }))
    await expect(apiPost('/api/admin-users')).rejects.toMatchObject({
      status: 503,
      message: 'HTTP 503',
    })
    fetchMock.mockResolvedValueOnce(new Response(null, { status: 204 }))
    await expect(apiDelete('/api/admin-users/a')).resolves.toBeNull()
  })

  it('一次性管理令牌移出地址栏并在后续请求复用', async () => {
    window.history.replaceState(null, '', '/accounts?admin_token=test-only-token&task_id=task')
    fetchMock.mockImplementation(() => Promise.resolve(new Response('{}')))
    try {
      await apiGet('/api/accounts')
      expect(window.location.search).toBe('?task_id=task')
      await apiGet('/api/admin-users')
      for (const call of fetchMock.mock.calls) {
        expect(call[1].headers).toMatchObject({ 'X-MediaGen-Admin-Token': 'test-only-token' })
      }
    } finally {
      window.localStorage.removeItem('mediagen_admin_token')
      window.history.replaceState(null, '', '/')
    }
  })
})

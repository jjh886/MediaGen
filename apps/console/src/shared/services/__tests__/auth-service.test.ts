import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getAuthStatus, loginAdmin, logoutAdmin } from '../auth-service'

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('auth-service', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('读取管理台登录状态', async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({ data: { configured: true, authenticated: false } })
    )

    await expect(getAuthStatus()).resolves.toEqual({
      configured: true,
      authenticated: false,
    })
    expect(fetchMock).toHaveBeenCalledWith('/api/auth/status', {
      headers: { Accept: 'application/json' },
    })
  })

  it('提交管理员账号密码登录', async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({ data: { configured: true, authenticated: true, username: 'admin' } })
    )

    await expect(loginAdmin({ username: 'admin', password: 'secret' })).resolves.toMatchObject({
      authenticated: true,
      username: 'admin',
    })
    expect(fetchMock).toHaveBeenCalledWith('/api/auth/login', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ username: 'admin', password: 'secret' }),
    })
  })

  it('退出登录会调用服务端清理会话', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ ok: true }))

    await logoutAdmin()

    expect(fetchMock).toHaveBeenCalledWith('/api/auth/logout', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: undefined,
    })
  })
})

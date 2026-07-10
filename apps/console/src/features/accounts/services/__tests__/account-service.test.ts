import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Account } from '@/shared/types/account'
import {
  addAccountByLogin,
  batchRegister,
  checkAccountsLogin,
  deleteAccount,
  fetchAccounts,
  startAccountLoginSession,
  toggleAccount,
  updateAccount,
} from '../account-service'

const account: Account = {
  id: 'acc-1',
  platform: 'codex',
  name: 'Codex',
  email: 'codex@example.com',
  avatar_url: '',
  tier: 'free',
  login_state: 'available',
  used_today: 0,
  daily_limit: 100,
  used_total: 0,
  total_limit: 1000,
  concurrency_limit: 1,
  task_interval_seconds: 0,
  proxy_url: 'socks5://127.0.0.1:1080',
  browser_os: 'windows',
  browser_version: '126',
  api_key: 'user-key',
  profile_dir: '/profiles/acc-1',
  created_at: '2026-06-29T00:00:00Z',
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function parseBody(callIndex: number): Record<string, unknown> {
  const init = fetchMock.mock.calls[callIndex]?.[1] as RequestInit | undefined
  return JSON.parse(String(init?.body ?? '{}')) as Record<string, unknown>
}

const fetchMock = vi.fn()

describe('account-service', () => {
  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('按平台请求账号列表', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: [account] }))

    const accounts = await fetchAccounts('codex')

    expect(fetchMock).toHaveBeenCalledWith('/api/accounts?platform=codex', {
      headers: { Accept: 'application/json' },
    })
    expect(accounts).toEqual([account])
  })

  it('更新账号配置时保留 snake_case 字段给服务端', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: account }))

    await updateAccount('acc-1', {
      tier: 'pro',
      concurrency_limit: 3,
      daily_limit: 200,
      total_limit: 2000,
      task_interval_seconds: 15,
      proxy_url: 'socks5://proxy.example:1080',
      browser_os: 'macos',
      browser_version: '127',
      api_key: 'key-2',
    })

    expect(fetchMock).toHaveBeenCalledWith('/api/accounts/acc-1', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: expect.any(String),
    })
    expect(parseBody(0)).toMatchObject({
      tier: 'pro',
      concurrency_limit: 3,
      daily_limit: 200,
      total_limit: 2000,
      task_interval_seconds: 15,
      proxy_url: 'socks5://proxy.example:1080',
      browser_os: 'macos',
      browser_version: '127',
      api_key: 'key-2',
    })
  })

  it('删除和禁用账号都走服务端 API', async () => {
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ ok: true }))
      .mockResolvedValueOnce(jsonResponse({ data: account }))

    await deleteAccount('acc-1')
    await toggleAccount('acc-1', true)

    expect(fetchMock).toHaveBeenNthCalledWith(1, '/api/accounts/acc-1', {
      method: 'DELETE',
      headers: { Accept: 'application/json' },
    })
    expect(fetchMock).toHaveBeenNthCalledWith(2, '/api/accounts/acc-1/toggle', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ disabled: true }),
    })
  })

  it('创建添加账号会话时使用插件 login_url', async () => {
    const session = { id: 'session-1', platform: 'codex', status: 'active' }
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({
          data: [{ platform_id: 'codex', login_url: 'https://login.example/codex' }],
        })
      )
      .mockResolvedValueOnce(jsonResponse({ ok: true, session }))

    const result = await startAccountLoginSession('codex')

    expect(result).toEqual(session)
    expect(parseBody(1)).toMatchObject({
      purpose: 'add_account',
      platform: 'codex',
      start_url: 'https://login.example/codex',
      account_id: null,
      width: 1365,
      height: 768,
    })
  })

  it('创建添加账号会话时可以带账号级代理', async () => {
    const session = { id: 'session-1', platform: 'codex', status: 'active' }
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ data: [{ platform_id: 'codex' }] }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, session }))

    await startAccountLoginSession('codex', undefined, 'socks5h://u:p@proxy.example:1080')

    expect(parseBody(1)).toMatchObject({
      purpose: 'add_account',
      metadata: { proxy_server: 'socks5h://u:p@proxy.example:1080' },
    })
  })

  it('创建添加账号会话时选择保存代理只传 proxy_id', async () => {
    const session = { id: 'session-1', platform: 'codex', status: 'active' }
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ data: [{ platform_id: 'codex' }] }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, session }))

    await startAccountLoginSession('codex', undefined, '', 'proxy-1')

    expect(parseBody(1)).toMatchObject({
      purpose: 'add_account',
      proxy_id: 'proxy-1',
    })
    expect(parseBody(1)).not.toHaveProperty('metadata.proxy_server')
  })

  it('创建添加账号会话时可以带浏览器环境', async () => {
    const session = { id: 'session-1', platform: 'codex', status: 'active' }
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ data: [{ platform_id: 'codex' }] }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, session }))

    await startAccountLoginSession('codex', undefined, '', undefined, {
      browser_os: 'macos',
      browser_version: '127',
    })

    expect(parseBody(1)).toMatchObject({
      purpose: 'add_account',
      metadata: { browser_os: 'macos', browser_version: '127' },
    })
  })

  it('重新登录会话会复用账号 profile_dir 并让后端兜底账号代理', async () => {
    const session = { id: 'session-2', platform: 'codex', status: 'active' }
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ data: [{ platform_id: 'codex' }] }))
      .mockResolvedValueOnce(jsonResponse({ data: [account] }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, session }))

    await startAccountLoginSession('codex', 'acc-1')

    expect(parseBody(2)).toMatchObject({
      purpose: 'relogin_account',
      platform: 'codex',
      account_id: 'acc-1',
      profile_dir: '/profiles/acc-1',
      metadata: {
        browser_os: 'windows',
        browser_version: '126',
      },
    })
  })

  it('重新登录会话会默认带上保存代理 id', async () => {
    const session = { id: 'session-2', platform: 'codex', status: 'active' }
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ data: [{ platform_id: 'codex' }] }))
      .mockResolvedValueOnce(jsonResponse({ data: [{ ...account, proxy_id: 'proxy-1' }] }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, session }))

    await startAccountLoginSession('codex', 'acc-1')

    expect(parseBody(2)).toMatchObject({
      purpose: 'relogin_account',
      account_id: 'acc-1',
      proxy_id: 'proxy-1',
    })
    expect(parseBody(2).metadata).not.toHaveProperty('proxy_server')
  })

  it('重新登录选择本次直连时会显式阻止后端补回旧代理', async () => {
    const session = { id: 'session-direct', platform: 'codex', status: 'active' }
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ data: [{ platform_id: 'codex' }] }))
      .mockResolvedValueOnce(
        jsonResponse({
          data: [{ ...account, proxy_id: 'proxy-1', proxy_url: 'socks5h://proxy.example:1080' }],
        })
      )
      .mockResolvedValueOnce(jsonResponse({ ok: true, session }))

    await startAccountLoginSession('codex', 'acc-1', '', '', undefined, 'direct')

    expect(parseBody(2)).toMatchObject({
      purpose: 'relogin_account',
      account_id: 'acc-1',
      proxy_id: null,
      metadata: { proxy_mode: 'direct' },
    })
    expect(parseBody(2).metadata).not.toHaveProperty('proxy_server')
  })

  it('添加账号、检查登录态和批量注册都只调用服务端', async () => {
    const session = { id: 'session-3', platform: 'codex', status: 'active' }
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ data: [{ platform_id: 'codex' }] }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, session }))
      .mockResolvedValueOnce(jsonResponse({ checked: 3, expired: 1 }))
      .mockResolvedValueOnce(jsonResponse({ ok: true }))

    await expect(addAccountByLogin('codex')).resolves.toBeNull()
    await expect(checkAccountsLogin()).resolves.toEqual({ checked: 3, expired: 1 })
    await batchRegister('codex', 2, 'pwd', '1990-01-01')

    expect(fetchMock).toHaveBeenNthCalledWith(3, '/api/accounts/check-login', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
    })
    expect(parseBody(3)).toEqual({
      platform: 'codex',
      count: 2,
      password: 'pwd',
      birthday: '1990-01-01',
    })
  })
})

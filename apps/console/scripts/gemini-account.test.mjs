import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import { describe, expect, it, vi } from 'vitest'

const source = (name) => readFileSync('../../plugins/gemini/scripts/' + name, 'utf8')
const identity = {
  Lz8rbb: true,
  S06Grb: '123456789012345678901',
  oPEP7c: 'test@example.com',
  SNlM0e: 'xsrf',
  cfb2h: 'build',
}
const status = () => {
  const data = Array(25).fill(null)
  data[15] = []
  data[16] = [8]
  data[17] = []
  return data
}
const usage = (
  pools = [
    {
      poolType: 'GXU_FIVE_HOURLY',
      percentageUsed: 0.27,
      gxu: { windowRefreshesAt: '2026-11-13T05:13:56.318980Z' },
    },
    {
      poolType: 'GXU_WEEKLY',
      percentageUsed: 0.28113717,
      gxu: { windowRefreshesAt: '2026-11-17T09:13:56.319154Z' },
    },
  ]
) => JSON.stringify([{ data: { viewer: { rpcs: { getUsageInfo: { pools } } } } }])
function runtime(data = identity) {
  const fetch = vi.fn(async (url) => {
    if (String(url).includes('/graphql/')) return { ok: true, text: async () => usage() }
    const rpc = new URL(url).searchParams.get('rpcids')
    const value =
      rpc === 'otAQ7b' ? status() : rpc === 'VxUbXb' ? [null, null, null, 0] : [false, 0, []]
    return { ok: true, text: async () => JSON.stringify([['wrb.fr', rpc, JSON.stringify(value)]]) }
  })
  const context = vm.createContext({
    window: { WIZ_global_data: data },
    location: {
      hostname: 'gemini.google.com',
      origin: 'https://gemini.google.com',
      pathname: '/app',
    },
    URL,
    URLSearchParams,
    AbortSignal,
    fetch,
    host: { call: vi.fn() },
  })
  vm.runInContext(source('account.js'), context)
  return { context, fetch }
}

describe('Gemini 登录身份和账号资料', () => {
  it.each([
    { SNlM0e: 'anonymous-token' },
    { ...identity, Lz8rbb: false },
    { ...identity, oPEP7c: '' },
    { ...identity, S06Grb: '' },
  ])('匿名 token、未登录标记和未加载完整的身份都不能结束登录', async (data) => {
    const { context, fetch } = runtime(data)
    expect(JSON.parse(await vm.runInContext(source('check_login.js'), context)).logged_in).toBe(
      false
    )
    expect(fetch).not.toHaveBeenCalled()
  })
  it('登录成功返回真实身份、Pro 类型和未知总量的额度状态', async () => {
    const { context } = runtime()
    const result = JSON.parse(await vm.runInContext(source('check_login.js'), context))
    expect(result).toMatchObject({
      logged_in: true,
      email: 'test@example.com',
      user_id: identity.S06Grb,
      tier: 'Google AI Pro',
    })
    expect(result.quota_windows.map((row) => row.name)).toEqual(['5 小时额度', '周额度'])
    expect(result.quota_windows[0].remaining_count).toBeUndefined()
    expect(result.quota_windows[0].remaining_percent).toBe(73)
  })
  it('身份接口失败不接受成功，额度接口失败保留已确认身份但不清空旧额度', async () => {
    const { context, fetch } = runtime()
    fetch.mockResolvedValueOnce({ ok: false, status: 503 })
    expect((await context.geminiBrowserAccount()).logged_in).toBeNull()
    const normal = fetch.getMockImplementation()
    fetch.mockImplementation((url) =>
      String(url).includes('/graphql/') ? { ok: false, status: 503 } : normal(url)
    )
    const result = await context.geminiBrowserAccount()
    expect(result.logged_in).toBe(true)
    expect(result.quota_windows).toBeUndefined()
  })
  it('采集阶段再次核验身份，不保存空邮箱或匿名凭据', async () => {
    const { context } = runtime({ SNlM0e: 'anonymous-token' })
    context.apiLoginFingerprint = vi.fn()
    await expect(vm.runInContext(source('capture_auth.js'), context)).rejects.toThrow('身份')
    expect(context.apiLoginFingerprint).not.toHaveBeenCalled()
  })
  it('API 刷新不能靠缓存邮箱和匿名 XSRF token 通过', () => {
    const { context } = runtime()
    vm.runInContext(source('auth.js'), context)
    context.geminiRequest = () => ({ body_text: '"Lz8rbb":false,"SNlM0e":"anonymous-token"' })
    expect(() => context.geminiInitialize({ auth: { email: 'stale@example.com' } })).toThrow(
      'LOGIN_EXPIRED'
    )
  })
  it('会员类型来自账号权益，额度未知不生成虚假数值', () => {
    const { context } = runtime()
    for (const [products, features, tier] of [
      [[16], [], 'Google AI Ultra'],
      [[], [198], 'Google AI Plus'],
      [[], [], 'Free'],
      [[21], [], 'Google Workspace'],
    ]) {
      const data = status()
      data[16] = products
      data[17] = features
      expect(context.geminiAccountProfile({ email: 'test@example.com' }, data).tier).toBe(tier)
    }
    expect(context.geminiUsageWindows(usage([]))).toEqual([])
    expect(
      context.geminiUsageWindows(
        usage([
          { poolType: 'GXU_WEEKLY', percentageUsed: 0.1 },
          { poolType: 'NEW_UPSTREAM_POOL', percentageUsed: 0.4 },
        ])
      )
    ).toEqual([
      { name: '周额度', remaining_percent: 90 },
      { name: 'NEW_UPSTREAM_POOL', remaining_percent: 60 },
    ])
    expect(context.geminiUsageWindows(")]}'\n" + usage())).toMatchObject([
      { remaining_percent: 73, reset_at: '2026-11-13T05:13:56.318Z' },
      { remaining_percent: 72, reset_at: '2026-11-17T09:13:56.319Z' },
    ])
    expect(
      context
        .geminiUsageWindows(usage())
        .every((row) => !row.task_kind && row.remaining_count === undefined)
    ).toBe(true)
    expect(() => context.geminiUsageWindows('[]')).toThrow('未返回统一用量')
    expect(() => context.geminiUsageWindows(usage([{ poolType: 'GXU_FIVE_HOURLY' }]))).toThrow(
      '不完整'
    )
  })
  it('头像与显示名来自当前账号个人资料，拒绝其他账号和非官方头像地址', () => {
    const { context } = runtime()
    const person = [
      identity.S06Grb,
      null,
      [[null, '测试用户']],
      [[null, 'https://lh3.googleusercontent.com/a/avatar=s96']],
    ]
    const data = [[['me', 1, person]]]
    expect(context.geminiAccountDetails({ user_id: identity.S06Grb }, data)).toEqual({
      username: '测试用户',
      avatar_url: person[3][0][1],
    })
    expect(context.geminiAccountDetails({ user_id: 'different' }, data)).toEqual({})
    person[3][0][1] = 'https://lh3.googleusercontent.com.attacker.test/a'
    expect(
      context.geminiAccountDetails({ user_id: identity.S06Grb }, data).avatar_url
    ).toBeUndefined()
  })
  it('API 额度查询使用网页 Cookie 鉴权，失败时不能声称刷新成功', () => {
    const { context } = runtime()
    const session = {
      auth: { user_id: identity.S06Grb, email: identity.oPEP7c },
      user_status: status(),
    }
    Object.assign(context, {
      apiSessionRead: () => session,
      geminiInitialize: (x) => x,
      geminiCatalog: () => [{}],
      geminiRpc: () => [],
      geminiBase: () => 'https://gemini.google.com',
      geminiRequest: (_session, url, options) => {
        expect(url).toContain('/graphql/schemas/GEMINI_WEB_GRAPHQL/executeQuery')
        expect(options.authorize).toBe(false)
        expect(options.headers['sec-fetch-site']).toBe('same-origin')
        expect(options.headers['sec-fetch-mode']).toBe('cors')
        expect(options.headers['sec-fetch-dest']).toBe('empty')
        return { body_text: usage() }
      },
      apiSessionHealthError: () => ({ status: 'inconclusive', logged_in: null }),
    })
    vm.runInContext(source('check_account.js'), context)
    expect(context.main({})).toMatchObject({
      status: 'available',
      quota_windows: [
        { name: '5 小时额度', remaining_percent: 73 },
        { name: '周额度', remaining_percent: 72 },
      ],
    })
    context.geminiRequest = () => {
      throw new Error('temporary_unavailable: Gemini HTTP 403')
    }
    expect(context.main({})).toMatchObject({ status: 'inconclusive', logged_in: null })
    expect(context.main({}).reason).toContain('403')
  })
})

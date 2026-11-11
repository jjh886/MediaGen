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
function runtime(data = identity) {
  const fetch = vi.fn(async (url) => {
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
    expect(result.quota_windows[0].name).toContain('未达上限')
    expect(result.quota_windows[0].remaining_count).toBeUndefined()
    expect(result.quota_windows[0].remaining_percent).toBeUndefined()
  })
  it('身份接口失败不接受成功，额度接口失败保留已确认身份但不清空旧额度', async () => {
    const { context, fetch } = runtime()
    fetch.mockResolvedValueOnce({ ok: false, status: 503 })
    expect((await context.geminiBrowserAccount()).logged_in).toBeNull()
    fetch
      .mockResolvedValueOnce({
        ok: true,
        text: async () => JSON.stringify([['wrb.fr', 'otAQ7b', JSON.stringify(status())]]),
      })
      .mockResolvedValueOnce({ ok: false, status: 503 })
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
    const reset = Math.floor(Date.now() / 1000) + 3600
    expect(context.geminiQuotaWindows([], [true, 0, [reset, 0]])[0]).toMatchObject({
      remaining_count: 0,
      reset_at: new Date(reset * 1000).toISOString(),
    })
    expect(
      context
        .geminiQuotaWindows([], [])
        .every((row) => row.remaining_count === undefined && row.remaining_percent === undefined)
    ).toBe(true)
    expect(
      context.geminiQuotaWindows([], [], [[[[null, 7], 2, 65, [reset, 0], 2400, 829]]])[0]
    ).toMatchObject({
      name: '图片额度',
      remaining_percent: 35,
      reset_at: new Date(reset * 1000).toISOString(),
    })
  })
})

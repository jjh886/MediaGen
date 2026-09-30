import { describe, expect, it } from 'vitest'
import { createHash, createHmac } from 'node:crypto'
import { scripts } from './platform-api-test-helper.mjs'

describe('DeepSeek 客户端会话协议', () => {
  const sources = [
    '_shared/api_session.js',
    'deepseek-api/scripts/auth.js',
    'deepseek-api/scripts/lifecycle.js',
  ]
  it('先确认设备和身份，使用服务端配置，仅报告实际读取的配置', () => {
    const calls = []
    const session = {
      auth_path: '/profile/auth.json',
      cookies: [],
      auth: {
        access_token: 'token',
        device_id: 'device',
        fingerprint: {},
        hif: {
          leim: { value: 'leim', refresh_at: Date.now() + 100000 },
          dliq: { refresh_at: Date.now() + 100000 },
        },
      },
    }
    const run = scripts(sources, {
      call: (action, params) => {
        if (action === 'file.write_json') return {}
        calls.push(params)
        const path = new URL(params.url).pathname
        const data = path.endsWith('/users/current')
          ? { id: 'user' }
          : path.endsWith('/client/settings')
            ? {
                settings: {
                  completion_request_timeout_ms: { id: 1, value: 90000 },
                  unused: { id: 2, value: 1 },
                },
              }
            : {}
        return {
          ok: true,
          status: 200,
          headers: path.endsWith('/client/settings')
            ? { 'x-settings-token': 'settings-token', 'x-fetch-after-sec': '60' }
            : {},
          body: { code: 0, data: { biz_code: 0, biz_data: data } },
        }
      },
    })
    run(
      'deepseekInitializeSession(session); timeout=deepseekSetting(session, "completion_request_timeout_ms",60000);deepseekReportSettings(session)',
      { session }
    )
    expect(run('timeout')).toBe(90000)
    expect(calls[0].body).toEqual({ device_id: 'device', device_model: '' })
    expect(calls[2].method).toBe('GET')
    expect(calls[2].headers['content-type']).toBeUndefined()
    expect(calls.at(-1).body).toEqual({ settings_ids: [1], did: 'device', sso_id: 'user' })
    expect(calls.at(-1).headers['x-settings-token']).toBe('settings-token')
    const headers = run('deepseekHeaders(session,"/api/v0/chat/completion")')
    expect(headers['x-hif-leim']).toBe('leim')
    expect(headers['x-settings-token']).toBeUndefined()
    expect(Object.keys(session)).not.toContain('usedSettings')
    expect(session.auth.device_checked).toBe(true)
  })

  it('HIF 不携带登录凭据，按 TTL 更新；失败保留旧值并退避，不阻断生成', () => {
    const calls = []
    const session = {
      auth_path: '/profile/auth.json',
      cookies: [],
      settings: {},
      usedSettings: new Set(),
      auth: {
        access_token: 'private-token',
        device_id: 'device',
        fingerprint: {},
        settings_token: 'private-settings',
        hif: { dliq: { value: 'old-dliq', refresh_at: 0 } },
      },
    }
    const run = scripts(sources, {
      call: (action, params) => {
        if (action === 'file.write_json') return {}
        calls.push(params)
        if (params.url.includes('hif-dliq')) return { ok: false, status: 503 }
        if (params.method === 'OPTIONS') return { ok: true, status: 204 }
        return {
          ok: true,
          status: 200,
          headers: { 'x-hif-ttl': '600' },
          body: { code: 0, data: { biz_code: 0, biz_data: { value: 'new-leim' } } },
        }
      },
    })
    run('deepseekRefreshHif(session)', { session })
    for (const request of calls) {
      expect(request.headers.authorization).toBeUndefined()
      expect(request.headers.cookie).toBeUndefined()
      expect(request.headers['x-settings-token']).toBeUndefined()
      expect(request.headers['x-device-id']).toBeUndefined()
    }
    expect(session.auth.hif.leim.value).toBe('new-leim')
    expect(session.auth.hif.leim.refresh_at).toBeGreaterThan(Date.now() + 599000)
    expect(session.auth.hif.dliq.value).toBe('old-dliq')
    expect(session.auth.hif.dliq.retry_delay).toBe(1000)
    run('deepseekRefreshHif(session)')
    expect(calls).toHaveLength(3)
  })

  it('辅助配置不可用时使用成功缓存，明确登录失效仍进入重新登录流程', () => {
    const session = {
      auth: { client_settings: { setting: { id: 1, value: 15 } } },
      usedSettings: new Set(),
    }
    const run = scripts(['deepseek-api/scripts/lifecycle.js'])
    run(
      'deepseekRequest=()=>{throw new Error("AUTH_INCONCLUSIVE: HTTP 503")};deepseekLoadSettings(session)',
      { session }
    )
    expect(run('deepseekSetting(session,"setting",1)')).toBe(15)
    run('deepseekRequest=()=>{throw new Error("LOGIN_EXPIRED: expired")}')
    expect(() => run('deepseekLoadSettings(session)')).toThrow('LOGIN_EXPIRED:')
  })

  it('仅登录令牌错误要求重新登录，业务错误和地区限制保留真实失败原因', () => {
    const session = {
      auth_path: '/profile/auth.json',
      cookies: [],
      auth: { access_token: 'token', fingerprint: {} },
    }
    for (const [body, message] of [
      [{ code: 40003 }, 'LOGIN_EXPIRED:'],
      [{ code: 0, data: { biz_code: 40003 } }, 'platform_rejected:'],
      [{ code: 40029, data: { biz_code: 40003 } }, 'platform_rejected: DeepSeek 错误码 40029'],
    ]) {
      const run = scripts(sources, {
        call: () => ({ ok: true, status: 200, headers: {}, body }),
      })
      expect(() =>
        run('deepseekRequest(session,"/users/current",undefined,"GET")', { session })
      ).toThrow(message)
    }
  })
})

describe('豆包账号心跳独立签名', () => {
  const sources = ['doubao-api/scripts/upload.js', 'doubao-api/scripts/passport.js']
  function cryptoHost(action, params) {
    if (action === 'crypto.base64_encode_utf8')
      return { value: Buffer.from(params.text).toString('base64') }
    if (action === 'crypto.base64_decode_latin1')
      return { value: Buffer.from(params.text, 'base64').toString('latin1') }
    if (action === 'crypto.digest_bytes')
      return {
        value: createHash('sha256')
          .update(Buffer.from(params.data_base64, 'base64'))
          .digest('base64'),
      }
    if (action === 'crypto.hmac_sha256')
      return {
        value: createHmac('sha256', Buffer.from(params.key_base64, 'base64'))
          .update(Buffer.from(params.data_base64, 'base64'))
          .digest('base64'),
      }
    throw new Error('Unexpected action')
  }
  it('按当前日期生成 HMAC，重新计算摘要与参数列表，不复用旧抓包签名', () => {
    const run = scripts(sources, { call: cryptoHost })
    const session = {
      cookies: [],
      auth: {
        passport_app_key: '2094abfcccefaeaffd24c622475fba10',
        passport_requests: {
          '/passport/token/beat/web/': {
            passport_jssdk_version: '3.1.6',
            account_sdk_source_info: 'captured-fingerprint',
            aid: '497858',
            p_js_t: 'pro',
            p_bd: '1.0.1.20-alpha.08',
            p_ver: '1.1.3',
            p_zt: '0',
            sign: 'stale-sign',
            qs: 'stale-qs',
            a_bogus: 'stale-bogus',
            msToken: 'stale-token',
          },
        },
      },
    }
    run(
      'doubaoWriteScriptCookie=()=>{};query=doubaoPassportQuery(session,"/passport/token/beat/web/")',
      { session }
    )
    const q = Object.fromEntries(new URLSearchParams(run('query')))
    const { sign, qs, ...unsigned } = q
    expect(q.a_bogus).toBeUndefined()
    expect(q.msToken).toBeUndefined()
    const keys = Object.keys(unsigned).sort().slice(0, 10)
    const digest = createHash('sha256')
      .update(
        keys.map((key) => key + '=' + unsigned[key]).join('&') +
          '&&app_key=2094abfcccefaeaffd24c622475fba10'
      )
      .digest('hex')
    expect(sign).toBe(digest)
    expect(qs).toBe([...Buffer.from(keys.join(','))].map((v) => (v ^ 5).toString(16)).join(''))
    const prk = createHmac('sha256', q.ts).update('2094abfcccefaeaffd24c622475fba10').digest()
    const key = createHmac('sha256', prk)
      .update(Buffer.from([1]))
      .digest()
    const signature = createHmac('sha256', key)
      .update('aid=497858&path=/passport/token/beat/web/&ts=' + q.ts)
      .digest('hex')
    expect(session.passportRequest.headers['x-tt-passport-aid-sign']).toBe(signature)
    expect(Object.keys(session)).not.toContain('passportRequest')
  })

  it('按真实十分钟和十五分钟间隔续期，401 不作为辅助网络失败吞掉', () => {
    const calls = []
    const run = scripts(['doubao-api/scripts/passport.js'])
    run('now=0;Date.now=()=>now;doubaoRequest=request;session={}', {
      request: (_session, path, _body, method) => {
        calls.push([path, method])
        return {}
      },
    })
    run('doubaoStartHeartbeats(session);now=599999;doubaoHeartbeatTick(session)')
    expect(calls).toHaveLength(2)
    run('now=600000;doubaoHeartbeatTick(session)')
    expect(calls.at(-1)).toEqual(['/passport/token/beat/web/', 'GET'])
    expect(run('session.heartbeatScene')).toBe('polling')
    run('now=900000;doubaoHeartbeatTick(session)')
    expect(calls.at(-1)).toEqual(['/passport/doubao/security_token_beat/', 'POST'])
    run('now=1800000;doubaoRequest=()=>({error_code:401})')
    expect(() => run('doubaoHeartbeatTick(session)')).toThrow('LOGIN_EXPIRED:')
  })

  it('多账号响应更新掩码 Cookie，后续请求恢复原始头值', () => {
    let cookie
    const run = scripts(sources, { call: cryptoHost })
    run('doubaoWriteScriptCookie=write;doubaoPassportRefresh({}, result)', {
      result: { headers: { 'x-tt-multi-sids': '{"user":"sid"}' } },
      write: (_session, line) => {
        cookie = line
      },
    })
    const masked = decodeURIComponent(cookie.split(';')[0].slice('x-tt-multi-sids='.length))
    expect(run('doubaoPassportUnmask(masked)', { masked })).toBe('{"user":"sid"}')
    expect(cookie).toContain('max-age=2592000')
  })
})

describe('豆包编号协议', () => {
  it('使用安全随机的十六字节十进制拼接生成临时会话编号', () => {
    const bytes = Buffer.from([1, 230, 44, 56, 78, 90, 12, 34, 56, 78, 90, 12, 34, 56, 78, 90])
    const run = scripts(['doubao-api/scripts/conversation.js'], {
      call: (action, params) => {
        if (action === 'crypto.random_base64url') {
          expect(params.bytes).toBe(16)
          return { value: bytes.toString('base64url') }
        }
        if (action === 'crypto.base64_decode_latin1')
          return { value: Buffer.from(params.text, 'base64').toString('latin1') }
        throw new Error('Unexpected action')
      },
    })
    expect(run('doubaoLocalConversationId()')).toBe('local_' + [...bytes].join('').slice(0, 16))
  })
})

const DOUBAO_PASSPORT_PATHS = ['/passport/token/beat/web/', '/passport/doubao/security_token_beat/']

function doubaoPassportQuery(session, path) {
  const template = session.auth.passport_requests?.[path]
  const appKey = session.auth.passport_app_key
  if (
    !template?.passport_jssdk_version ||
    !template.account_sdk_source_info ||
    typeof appKey !== 'string' ||
    !/^[a-f0-9]{32}$/.test(appKey)
  )
    throw new Error(
      'AUTH_PROTOCOL_INCOMPLETE: 豆包账号登录资料需要更新。请在账号设置中点击重新登录，采集新版接口协议后重试。'
    )
  const params = { ...template }
  for (const name of [
    'sign',
    'qs',
    'msToken',
    'a_bogus',
    'ts',
    'p_ts',
    'p_no',
    'biz_trace_id',
    'scene',
  ])
    delete params[name]
  const now = new Date()
  params.ts = String(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate(), 12) / 1000)
  if (!session.passportTraces) Object.defineProperty(session, 'passportTraces', { value: {} })
  if (!session.passportTraces[path]) {
    let time = Date.now()
    session.passportTraces[path] = 'xxxxxxxx'.replace(/x/g, () => {
      const value = ((time + Math.random() * 16) % 16) | 0
      time = Math.floor(time / 16)
      return value.toString(16)
    })
  }
  params.biz_trace_id = session.passportTraces[path]
  doubaoWriteScriptCookie(
    session,
    'biz_trace_id=' + params.biz_trace_id + ';path=/;domain=.doubao.com'
  )
  if (path === DOUBAO_PASSPORT_PATHS[0]) params.scene = session.heartbeatScene || 'boot'
  if (path === DOUBAO_PASSPORT_PATHS[1]) params.multi_login = 1
  if (params.p_js_t === 'pro') {
    params.p_ts = Date.now()
    const proof = {
      passport_jssdk_version: params.passport_jssdk_version,
      p_bd: params.p_bd,
      p_ca: '0',
      p_ts: params.p_ts,
      p_ver: params.p_ver,
      p_zt: params.p_zt,
    }
    params.p_no = doubaoHash(doubaoPassportCanonical(proof))
  }
  const names = Object.keys(params).sort().slice(0, 10)
  const selected = Object.fromEntries(names.map((name) => [name, params[name]]))
  params.sign = doubaoHash(doubaoPassportCanonical(selected) + '&&app_key=' + appKey)
  params.qs = names
    .join(',')
    .split('')
    .map((char) => (char.charCodeAt(0) ^ 5).toString(16))
    .join('')
  const extracted = doubaoHmac(doubaoBase64(params.ts), appKey)
  const key = doubaoHmac(extracted, '\x01')
  const headers = {
    'x-tt-passport-aid-sign': doubaoHex(
      doubaoHmac(key, 'aid=' + params.aid + '&path=' + path + '&ts=' + params.ts)
    ),
    'x-tt-passport-trace-id': params.biz_trace_id,
    'x-tt-passport-csrf-token':
      doubaoPassportCookie(session, 'passport_csrf_token') ||
      doubaoPassportCookie(session, 'passport_csrf_token_default'),
  }
  if (path === DOUBAO_PASSPORT_PATHS[1]) {
    const masked = doubaoPassportCookie(session, 'x-tt-multi-sids')
    if (masked) headers['x-tt-multi-sids'] = doubaoPassportUnmask(masked)
    headers['content-type'] = 'application/x-www-form-urlencoded'
  }
  Object.defineProperty(session, 'passportRequest', {
    value: { path, headers },
    configurable: true,
  })
  return Object.entries(params)
    .map(([key, value]) => encodeURIComponent(key) + '=' + encodeURIComponent(value))
    .join('&')
}

function doubaoPassportCanonical(params) {
  return Object.keys(params)
    .sort()
    .map(
      (key) =>
        key + '=' + (typeof params[key] === 'object' ? JSON.stringify(params[key]) : params[key])
    )
    .join('&')
}

function doubaoPassportCookie(session, name) {
  const cookie = session.cookies.find(
    (cookie) =>
      cookie.name === name &&
      !cookie.httpOnly &&
      ['doubao.com', 'www.doubao.com'].includes(String(cookie.domain).replace(/^\./, '')) &&
      (!cookie.expires || cookie.expires <= 0 || cookie.expires > Date.now() / 1000)
  )
  return cookie ? decodeURIComponent(cookie.value) : ''
}

function doubaoPassportMask(text) {
  const key = 'flow-web-mask-crypto-key'
  return text
    .split('')
    .map((char, index) =>
      String.fromCharCode(char.charCodeAt(0) ^ key.charCodeAt(index % key.length))
    )
    .join('')
}

function doubaoPassportUnmask(value) {
  const bytes = host.call('crypto.base64_decode_latin1', { text: value }).value
  const text = decodeURIComponent(
    bytes
      .split('')
      .map((char) => '%' + char.charCodeAt(0).toString(16).padStart(2, '0'))
      .join('')
  )
  return doubaoPassportMask(text)
}

function doubaoPassportRefresh(session, result) {
  const value = result.headers?.['x-tt-multi-sids']
  if (typeof value === 'string' && value) {
    doubaoWriteScriptCookie(
      session,
      'x-tt-multi-sids=' +
        encodeURIComponent(doubaoBase64(doubaoPassportMask(value))) +
        ';path=/;domain=.doubao.com;max-age=2592000'
    )
  }
}

function doubaoStartHeartbeats(session) {
  Object.defineProperty(session, 'heartbeatState', {
    value: { token: 0, security: 0 },
    writable: true,
  })
  doubaoHeartbeatTick(session)
}

function doubaoHeartbeatTick(session) {
  if (!session.heartbeatState) return
  const checks = [
    ['token', DOUBAO_PASSPORT_PATHS[0], 'GET', 600000],
    ['security', DOUBAO_PASSPORT_PATHS[1], 'POST', 900000],
  ]
  for (const [name, path, method, interval] of checks) {
    if (Date.now() < session.heartbeatState[name]) continue
    try {
      const data = doubaoRequest(session, path, undefined, method, 10000)
      if (!data || typeof data !== 'object' || Array.isArray(data))
        throw new Error('invalid_response: 豆包账号续期响应无效')
      if (data?.error_code) {
        if (Number(data.error_code) === 401) throw new Error('LOGIN_EXPIRED: 豆包账号续期失效')
        if (data.verify_decision || data.verify_center_decision_conf) {
          doubaoSaveVerification(session, {
            extra: { decision: data.verify_decision || data.verify_center_decision_conf },
          })
          throw new Error('verification_required: 豆包账号续期要求验证')
        }
        throw new Error('platform_rejected: 豆包账号续期错误码 ' + Number(data.error_code))
      }
      session.heartbeatState[name] = Date.now() + interval
    } catch (error) {
      if (!String(error?.message || '').startsWith('AUTH_INCONCLUSIVE:')) throw error
      session.heartbeatState[name] = Date.now() + 10000
    }
  }
  if (!Object.hasOwn(session, 'heartbeatScene'))
    Object.defineProperty(session, 'heartbeatScene', { value: 'polling' })
}

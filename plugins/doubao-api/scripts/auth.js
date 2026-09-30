const DOUBAO_ORIGIN = 'https://www.doubao.com'
const DOUBAO_ACCOUNT_ORIGIN = 'https://accounts.doubao.com'

function doubaoRequestOrigin(path) {
  return path.startsWith('/passport/') ? DOUBAO_ACCOUNT_ORIGIN : DOUBAO_ORIGIN
}

function doubaoQuery(session, path) {
  if (path.startsWith('/passport/')) {
    if (typeof DOUBAO_PASSPORT_PATHS !== 'undefined' && DOUBAO_PASSPORT_PATHS.includes(path))
      return doubaoPassportQuery(session, path)
    const account = session.auth.account_query
    if (!account?.account_sdk_source || !account.sdk_version)
      throw new Error('LOGIN_EXPIRED: 豆包账号协议参数缺失，请重新登录')
    return Object.entries(account)
      .map(([key, value]) => encodeURIComponent(key) + '=' + encodeURIComponent(value))
      .join('&')
  }
  const params = session.auth.protocol_query
  if (!params || !params.web_id || !params.fp)
    throw new Error('LOGIN_EXPIRED: 豆包网页身份参数缺失，请重新登录')
  const completion = path.startsWith('/chat/')
  const keys = Object.keys(params).filter((key) => completion || !['fp', 'tz_name'].includes(key))
  if (completion) keys.sort()
  return keys
    .map((key) => encodeURIComponent(key) + '=' + encodeURIComponent(params[key]))
    .join('&')
}

function doubaoHeaders(session, path = '/') {
  if (path.startsWith('/passport/')) {
    const headers = apiSessionHeaders(session, DOUBAO_ACCOUNT_ORIGIN, path)
    Object.assign(headers, {
      accept: '*/*',
      origin: DOUBAO_ORIGIN,
      referer: DOUBAO_ORIGIN + '/',
      'sec-fetch-site': 'same-site',
    })
    if (session.passportRequest?.path === path) {
      delete headers['content-type']
      Object.assign(headers, session.passportRequest.headers)
    }
    return headers
  }
  const headers = { ...apiSessionHeaders(session, DOUBAO_ORIGIN, path), 'agw-js-conv': 'str' }
  const csrf = session.cookies.find((cookie) => cookie.name === 'passport_csrf_token')?.value
  if (csrf) headers['x-tt-passport-csrf-token'] = csrf
  if (!headers.cookie) throw new Error('LOGIN_EXPIRED: 豆包登录 Cookie 已失效')
  return headers
}

function doubaoRequest(session, path, body, method, timeoutMs = 60000) {
  const request = doubaoSignedRequest(session, path, body, method || 'POST')
  if (path.startsWith('/passport/')) {
    const names = Object.keys(request.headers).filter((name) => name.startsWith('x-tt-'))
    if (request.headers['content-type'] === 'application/json') names.push('content-type')
    doubaoCorsPreflight(session, request, names, true)
  }
  const result = apiSessionRequireOk(
    host.call('http.request', {
      ...request,
      timeout_ms: timeoutMs,
    }),
    '豆包请求失败'
  )
  doubaoRefreshSecurity(session, result, path)
  doubaoCheckError(result.body, session)
  return result.body?.data ?? result.body
}

function doubaoCorsPreflight(session, request, headerNames, credentials = false) {
  const requestedHeaders = [...headerNames].sort().join(',')
  const key = request.method + ' ' + request.url + ' ' + requestedHeaders + ' ' + credentials
  if (session.corsPreflight?.key === key && session.corsPreflight.until > Date.now()) return
  const headers = doubaoHeaders(session, '/passport/')
  // 浏览器预检不携带 Cookie、客户端提示和业务请求头。
  for (const name of Object.keys(headers))
    if (!['accept', 'user-agent', 'origin', 'referer', 'sec-fetch-mode'].includes(name))
      delete headers[name]
  headers['access-control-request-method'] = request.method
  headers['access-control-request-headers'] = requestedHeaders
  const response = apiSessionRequireOk(
    host.call('http.request', {
      method: 'OPTIONS',
      url: request.url,
      headers,
      proxy: 'system',
      timeout_ms: 60000,
      retry: { max_attempts: 1 },
    }),
    '豆包跨域预检失败'
  )
  const result = response.headers || {}
  const allowedHeaders = String(result['access-control-allow-headers'])
    .toLowerCase()
    .split(/\s*,\s*/)
  if (
    (result['access-control-allow-origin'] !== DOUBAO_ORIGIN &&
      (credentials || result['access-control-allow-origin'] !== '*')) ||
    (credentials && result['access-control-allow-credentials'] !== 'true') ||
    !String(result['access-control-allow-methods'])
      .split(/\s*,\s*/)
      .includes(request.method) ||
    !headerNames.every(
      (name) =>
        allowedHeaders.includes(name) ||
        (!credentials && name !== 'authorization' && allowedHeaders.includes('*'))
    )
  )
    throw new Error('invalid_response: 豆包跨域预检未允许当前请求')
  const seconds = Number(result['access-control-max-age'] || 5)
  Object.defineProperty(session, 'corsPreflight', {
    configurable: true,
    value: {
      key,
      until:
        Date.now() + Math.min(7200, Math.max(0, Number.isFinite(seconds) ? seconds : 0)) * 1000,
    },
  })
}

function doubaoRequireVerified(session) {
  if (session.auth.verification_required)
    throw new Error('verification_required: 请在豆包登录窗口完成上次请求的验证后重试')
}

function doubaoSaveVerification(session, data) {
  if (!session) return
  const raw = data?.extra?.decision
  let decision
  try {
    decision = typeof raw === 'string' ? JSON.parse(raw) : raw
  } catch (_) {}
  if (decision?.type !== 'verify') decision = null
  host.call('file.write_json', {
    path: session.auth_path.slice(0, session.auth_path.lastIndexOf('/')) + '/verification.json',
    value: { decision, requires_manual_verification: true, captured_at: Date.now() },
  })
  session.auth.verification_required = true
  host.call('file.write_json', { path: session.auth_path, value: session })
}

function doubaoCheckError(data, session) {
  const code = Number(data?.code || 0)
  if (!code) return
  if ([401, 101, 10000, 10002].includes(code)) throw new Error('LOGIN_EXPIRED: 豆包登录已失效')
  if (code === 710022004) {
    doubaoSaveVerification(session, data)
    throw new Error('verification_required: 豆包需要在登录窗口完成验证')
  }
  const message = String(data.message || data.msg || '')
  if (/额度|quota|credit/i.test(message)) throw new Error('quota_exhausted: 豆包额度不足')
  if (/频繁|rate.?limit/i.test(message)) throw new Error('rate_limit_exceeded: 豆包请求过于频繁')
  throw new Error('platform_rejected: 豆包错误码 ' + code)
}

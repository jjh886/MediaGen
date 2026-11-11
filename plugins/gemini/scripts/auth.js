const GEMINI_ORIGIN = 'https://gemini.google.com'

function geminiBase(session) {
  const user = Number(session.auth.authuser || 0)
  if (!Number.isInteger(user) || user < 0 || user > 99)
    throw new Error('invalid_input: Google 账号序号无效')
  return GEMINI_ORIGIN + (user ? '/u/' + user : '')
}

function geminiHeaders(session, url, authorize = true) {
  const match = /^https:\/\/([^/?#]+)(\/[^?#]*)?/.exec(url)
  if (!match) throw new Error('invalid_input: Gemini 请求地址无效')
  const cookie = apiSessionCookies(session, match[1], match[2] || '/')
  const headers = {
    accept: '*/*',
    origin: GEMINI_ORIGIN,
    referer: geminiBase(session) + '/app',
    'x-goog-authuser': String(session.auth.authuser || 0),
    'x-same-domain': '1',
    ...(cookie ? { cookie } : {}),
  }
  if (authorize && match[1] === 'gemini.google.com') {
    const read = (name) =>
      session.cookies.find((c) => c.name === name && cookie.includes(c.name + '=' + c.value))?.value
    const primary = read('SAPISID') || read('__Secure-3PAPISID')
    if (!primary) throw new Error('LOGIN_EXPIRED: Gemini 缺少有效 Google 登录 Cookie')
    const time = Math.floor(Date.now() / 1000)
    headers.authorization = [
      ['SAPISIDHASH', primary],
      ['SAPISID1PHASH', read('__Secure-1PAPISID')],
      ['SAPISID3PHASH', read('__Secure-3PAPISID')],
    ]
      .filter(([, secret]) => secret)
      .map(([scheme, secret]) => {
        const data = host.call('crypto.base64_encode_utf8', {
          text: time + ' ' + secret + ' ' + GEMINI_ORIGIN,
        }).value
        const hash = host.call('crypto.digest_bytes', {
          algorithm: 'sha1',
          data_base64: data,
        }).value
        const bytes = host.call('crypto.base64_decode_latin1', { text: hash }).value
        return (
          scheme +
          ' ' +
          time +
          '_' +
          Array.from(bytes, (c) => c.charCodeAt(0).toString(16).padStart(2, '0')).join('')
        )
      })
      .join(' ')
  }
  return headers
}

function geminiRequest(session, url, options = {}) {
  const response = host.call('http.request', {
    url,
    method: 'GET',
    proxy: 'system',
    timeout_ms: 60000,
    retry: { max_attempts: 1 },
    include_final_url: true,
    ...options,
    headers: { ...geminiHeaders(session, url, options.authorize !== false), ...options.headers },
  })
  const hostname = /^https:\/\/([^/]+)/.exec(url)?.[1]
  if (
    hostname &&
    (hostname === 'google.com' || hostname.endsWith('.google.com')) &&
    apiSessionUpdateCookies(session, response.headers, hostname, '/', 'google.com')
  ) {
    host.call('file.write_json', { path: session.auth_path, value: session })
  }
  geminiRequireOk(response)
  return response
}

function geminiRequireOk(response) {
  const status = Number(response?.status || 0)
  const location = String(response?.final_url || response?.headers?.location || '')
  if (status === 401 || /accounts\.google\.com\/(?:ServiceLogin|v\d\/signin)/.test(location))
    throw new Error('LOGIN_EXPIRED: Google 登录已失效')
  if (
    /google\.com\/sorry/.test(location) ||
    /<form[^>]+(?:captcha|challenge)/i.test(response?.body_text || '')
  )
    throw new Error('verification_required: Google 要求完成安全验证')
  if (response?.ok) return response
  const retry = Number(response?.headers?.['retry-after'])
  const suffix =
    Number.isFinite(retry) && retry > 0 ? '; retry after ' + Math.ceil(retry) + ' seconds' : ''
  const category =
    status === 429
      ? 'rate_limit_exceeded'
      : status === 400
        ? 'invalid_input'
        : 'temporary_unavailable'
  throw new Error(category + ': Gemini HTTP ' + status + suffix)
}

function geminiPageField(html, key) {
  const match = html.match(new RegExp('"' + key + '"\\s*:\\s*("(?:[^"\\\\]|\\\\.)*"|true|false)'))
  if (!match) return ''
  try {
    return JSON.parse(match[1])
  } catch (_) {
    return ''
  }
}

function geminiInitialize(session) {
  let response = geminiRequest(session, geminiBase(session) + '/app', {
    authorize: false,
    headers: { accept: 'text/html' },
  })
  if (!geminiPageField(response.body_text || '', 'SNlM0e') && geminiRefreshCookies(session)) {
    response = geminiRequest(session, geminiBase(session) + '/app', {
      authorize: false,
      headers: { accept: 'text/html' },
    })
  }
  const html = response.body_text || ''
  const identity = geminiIdentity(
    Object.fromEntries(
      ['Lz8rbb', 'oPEP7c', 'S06Grb'].map((key) => [key, geminiPageField(html, key)])
    )
  )
  if (!identity) {
    if (geminiPageField(html, 'Lz8rbb') === false)
      throw new Error('LOGIN_EXPIRED: Gemini 返回未登录页面')
    throw new Error('AUTH_INCONCLUSIVE: Gemini 页面未返回完整账号身份')
  }
  Object.assign(session.auth, identity)
  const tokens = {
    at: geminiPageField(html, 'SNlM0e'),
    bl: geminiPageField(html, 'cfb2h'),
    push_id: geminiPageField(html, 'qKIAYe'),
    pctx: geminiPageField(html, 'Ylro7b'),
    api_key: geminiPageField(html, 'd2zJAe'),
    server_token: geminiPageField(html, 'PI9WOb'),
    session_id: geminiPageField(html, 'FdrFJe'),
  }
  if (!tokens.at)
    throw new Error('AUTH_INCONCLUSIVE: Gemini 页面未返回登录会话参数，请刷新账号登录状态')
  if (!/^boq_[a-z0-9-]+_\d{8}\.\d{2}_p\d+$/.test(tokens.bl))
    throw new Error('upstream_client_outdated: Gemini 页面构建版本无法识别')
  session.tokens = tokens
  return session
}

function geminiRefreshCookies(session) {
  if (Date.now() - Number(session.auth.cookie_refresh_attempt || 0) < 60000) return false
  session.auth.cookie_refresh_attempt = Date.now()
  host.call('file.write_json', { path: session.auth_path, value: session })
  const names = ['__Secure-1PSID', '__Secure-1PSIDTS']
  const cookie = apiSessionCookies(session, 'accounts.google.com')
    .split('; ')
    .filter((part) => names.includes(part.split('=')[0]))
    .join('; ')
  if (!cookie.includes('__Secure-1PSID=')) return false
  try {
    geminiRequest(session, 'https://accounts.google.com/RotateCookies', {
      method: 'POST',
      authorize: false,
      body_format: 'text',
      body: '[000,"-0000000000000000000"]',
      headers: {
        cookie,
        origin: 'https://accounts.google.com',
        referer: 'https://accounts.google.com/',
        'content-type': 'application/json',
      },
    })
    delete session.auth.cookie_refresh_error
    return true
  } catch (error) {
    // 轮换失败不等于整个账号失效；保留诊断，由后续网页登录检查确认状态。
    session.auth.cookie_refresh_error = String(error.message)
    return false
  } finally {
    host.call('file.write_json', { path: session.auth_path, value: session })
  }
}

function geminiRpc(session, rpc, args, headers = {}) {
  session.rpc_request_id = (session.rpc_request_id || Math.floor(Math.random() * 90000)) + 100000
  const url =
    geminiBase(session) +
    '/_/BardChatUi/data/batchexecute?rpcids=' +
    encodeURIComponent(rpc) +
    '&bl=' +
    encodeURIComponent(session.tokens.bl) +
    '&source-path=' +
    encodeURIComponent(session.source_path || '/app') +
    '&f.sid=' +
    encodeURIComponent(session.tokens.session_id || '') +
    '&hl=zh-CN&_reqid=' +
    session.rpc_request_id +
    '&rt=c'
  const response = geminiRequest(session, url, {
    method: 'POST',
    headers,
    body_format: 'form',
    body: {
      at: session.tokens.at,
      'f.req': JSON.stringify([[[rpc, JSON.stringify(args), null, 'generic']]]),
    },
  })
  const frames = geminiFrames(response.body_text)
  const row = frames.find((frame) => frame.rpc === rpc)
  if (!row) throw new Error('invalid_response: Gemini RPC ' + rpc + ' 缺少有效结果')
  return row.data
}

const CHATGPT_API_ORIGIN = 'https://chatgpt.com'
const CHATGPT_API_AUTH_FILE = 'auth.json'

function chatgptApiReadAuth(ctx) {
  const stored = host.call('file.read_json', {
    path: ctx.profile_dir + '/' + CHATGPT_API_AUTH_FILE,
  })
  if (!stored?.auth || !Array.isArray(stored?.cookies)) {
    throw new Error('LOGIN_EXPIRED: auth.json 缺少登录凭据，请重新登录 ChatGPT API。')
  }
  return stored
}

function chatgptApiCookieHeader(auth) {
  const now = Date.now() / 1000
  return auth.cookies
    .filter((cookie) => {
      const expires = Number(cookie?.expires || 0)
      return cookie?.name && cookie?.value && (expires <= 0 || expires > now)
    })
    .map((cookie) => cookie.name + '=' + cookie.value)
    .join('; ')
}

function chatgptApiSessionId() {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (char) => {
    const value = Math.floor(Math.random() * 16)
    return (char === 'x' ? value : (value & 3) | 8).toString(16)
  })
}

function chatgptApiRefreshSession(auth) {
  const cookie = chatgptApiCookieHeader(auth)
  if (!cookie) throw new Error('LOGIN_EXPIRED: 登录 Cookie 已过期，请重新登录 ChatGPT API。')
  auth.runtime_started_at = Date.now()
  chatgptApiNavigateHome(auth, cookie)
  const result = chatgptApiFetchSession(auth, cookie)
  chatgptApiValidateSession(result)
  return chatgptApiApplySession(auth, result.body)
}

// 请求、校验和写回分开处理，让各阶段的失败原因保持清晰。
function chatgptApiFetchSession(auth, cookie) {
  return host.call('http.request', {
    method: 'GET',
    url: CHATGPT_API_ORIGIN + '/api/auth/session',
    proxy: 'system',
    headers: {
      accept: 'application/json',
      cookie,
      ...chatgptApiBrowserHeaders(auth, 'same-origin'),
    },
    timeout_ms: 30000,
  })
}

function chatgptApiNavigateHome(auth, cookie) {
  const result = host.call('http.request', {
    method: 'GET',
    url: CHATGPT_API_ORIGIN + '/',
    proxy: 'system',
    headers: {
      ...chatgptApiBrowserHeaders(auth, 'none'),
      accept:
        'text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8',
      cookie,
      'sec-fetch-dest': 'document',
      'sec-fetch-mode': 'navigate',
      'sec-fetch-site': 'none',
      'sec-fetch-user': '?1',
      'upgrade-insecure-requests': '1',
    },
    timeout_ms: 60000,
  })
  if (!result.ok) {
    throw new Error('AUTH_INCONCLUSIVE: 打开 ChatGPT 首页失败：' + chatgptApiHttpSummary(result))
  }
  // 页面发布版本会变化，生成前从真实首页刷新构建号，避免长期复用登录时的旧值。
  const html = String(result.body_text || '')
  const fingerprint = chatgptApiFingerprint(auth)
  fingerprint.page_build = chatgptApiHtmlAttribute(html, 'data-build') || fingerprint.page_build
  fingerprint.page_sequence = chatgptApiHtmlAttribute(html, 'data-seq') || fingerprint.page_sequence
}

function chatgptApiHtmlAttribute(html, name) {
  const match = String(html || '').match(new RegExp(name + '=["\\\']([^"\\\']+)["\\\']'))
  return match?.[1] || ''
}

function chatgptApiValidateSession(result) {
  if (Number(result.status) === 401 || Number(result.status) === 403) {
    throw new Error('LOGIN_EXPIRED: ChatGPT 登录已失效，请重新登录。')
  }
  if (!result.ok || !result.body?.accessToken || !result.body?.account?.id) {
    throw new Error(
      'AUTH_INCONCLUSIVE: 刷新 ChatGPT session 失败：' + chatgptApiHttpSummary(result)
    )
  }
}

function chatgptApiApplySession(auth, session) {
  auth.auth.access_token = session.accessToken
  auth.auth.expires = session.expires || auth.auth.expires || ''
  auth.auth.account = session.account
  auth.auth.user = session.user || auth.auth.user || {}
  auth.runtime_session_id = chatgptApiSessionId()
  return auth
}

function chatgptApiFingerprint(auth) {
  return auth?.auth?.fingerprint || {}
}

function chatgptApiAccountId(auth) {
  return String(auth?.auth?.account?.id || '')
}

function chatgptApiBaseHeaders(auth, targetPath, targetRoute) {
  const fingerprint = chatgptApiFingerprint(auth)
  const headers = {
    ...chatgptApiBrowserHeaders(auth, 'same-origin'),
    accept: '*/*',
    authorization: 'Bearer ' + auth.auth.access_token,
    cookie: chatgptApiCookieHeader(auth),
    'oai-client-build-number': fingerprint.page_sequence || '',
    'oai-client-version': fingerprint.page_build || '',
    'oai-device-id': chatgptApiCookieValue(auth, 'oai-did'),
    'oai-language': fingerprint.language || 'en-US',
    'oai-session-id': auth.runtime_session_id || chatgptApiSessionId(),
    'x-openai-target-path': targetPath,
    'x-openai-target-route': targetRoute || targetPath,
  }
  return chatgptApiCompactObject(headers)
}

function chatgptApiBrowserHeaders(auth, fetchSite) {
  const fingerprint = chatgptApiFingerprint(auth)
  const sameOrigin = (fetchSite || 'same-origin') !== 'none'
  const headers = {
    'accept-encoding': 'gzip, deflate, br, zstd',
    'accept-language': chatgptApiAcceptLanguage(fingerprint.languages),
    origin: sameOrigin ? CHATGPT_API_ORIGIN : '',
    priority: 'u=1, i',
    referer: sameOrigin ? CHATGPT_API_ORIGIN + '/' : '',
    'sec-ch-ua':
      chatgptApiUaBrands(fingerprint.ua_brands) || chatgptApiFallbackUaBrands(fingerprint),
    'sec-ch-ua-arch': chatgptApiOptionalQuoted(fingerprint, 'ua_architecture'),
    'sec-ch-ua-bitness': chatgptApiOptionalQuoted(fingerprint, 'ua_bitness'),
    'sec-ch-ua-full-version': chatgptApiOptionalQuoted(fingerprint, 'ua_full_version'),
    'sec-ch-ua-full-version-list': chatgptApiUaBrands(fingerprint.ua_full_version_list),
    'sec-ch-ua-mobile': fingerprint.ua_mobile ? '?1' : '?0',
    'sec-ch-ua-model': chatgptApiOptionalQuoted(fingerprint, 'ua_model'),
    'sec-ch-ua-platform': chatgptApiQuoted(
      fingerprint.ua_platform || chatgptApiPlatformName(fingerprint.platform)
    ),
    'sec-ch-ua-platform-version': chatgptApiOptionalQuoted(fingerprint, 'ua_platform_version'),
    'sec-fetch-dest': 'empty',
    'sec-fetch-mode': 'cors',
    'sec-fetch-site': fetchSite || 'same-origin',
    'user-agent': fingerprint.user_agent || '',
  }
  return chatgptApiCompactObject(headers)
}

function chatgptApiOptionalQuoted(value, key) {
  return Object.prototype.hasOwnProperty.call(value || {}, key) ? chatgptApiQuoted(value[key]) : ''
}

function chatgptApiFallbackUaBrands(fingerprint) {
  const major = String(fingerprint.user_agent || '').match(/Chrome\/(\d+)/)?.[1] || ''
  if (!major) return ''
  return chatgptApiUaBrands([
    { brand: 'Not:A-Brand', version: '99' },
    { brand: 'Google Chrome', version: major },
    { brand: 'Chromium', version: major },
  ])
}

function chatgptApiPlatformName(platform) {
  const value = String(platform || '').toLowerCase()
  if (value.includes('mac')) return 'macOS'
  if (value.includes('win')) return 'Windows'
  if (value.includes('linux')) return 'Linux'
  return ''
}

function chatgptApiUaBrands(brands) {
  return (Array.isArray(brands) ? brands : [])
    .filter((item) => item?.brand && item?.version)
    .map((item) => chatgptApiQuoted(item.brand) + ';v=' + chatgptApiQuoted(item.version))
    .join(', ')
}

function chatgptApiQuoted(value) {
  return '"' + String(value || '').replace(/["\\]/g, '\\$&') + '"'
}

function chatgptApiAcceptLanguage(languages) {
  const values = Array.isArray(languages) && languages.length ? languages : ['en-US']
  return values
    .slice(0, 10)
    .map((value, index) => String(value) + (index ? ';q=' + Math.max(0.1, 1 - index / 10) : ''))
    .join(',')
}

function chatgptApiCookieValue(auth, name) {
  const match = auth.cookies.find((cookie) => cookie?.name === name)
  return String(match?.value || '')
}

function chatgptApiRequest(auth, method, path, options) {
  const config = options || {}
  const targetPath = config.target_path || path
  const headers = chatgptApiBaseHeaders(auth, targetPath, config.target_route || targetPath)
  Object.assign(headers, config.headers || {})
  const request = {
    method,
    url: CHATGPT_API_ORIGIN + path,
    proxy: 'system',
    headers,
    timeout_ms: config.timeout_ms || 30000,
  }
  if (config.body !== undefined) request.body = config.body
  if (config.body_format) request.body_format = config.body_format
  return host.call(config.sse ? 'http.sse_request' : 'http.request', request)
}

function chatgptApiRequireOk(result, operation) {
  if (result?.ok) return result
  const status = Number(result?.status || 0)
  if (status === 401 || status === 403) {
    throw new Error('LOGIN_EXPIRED: ' + operation + ' 返回 HTTP ' + status)
  }
  throw new Error(operation + ' 失败：' + chatgptApiHttpSummary(result))
}

function chatgptApiHttpSummary(result) {
  const status = Number(result?.status || 0)
  const body = result?.body
  const message =
    body?.detail || body?.error?.message || body?.error || body?.message || result?.body_text || ''
  return (status ? 'HTTP ' + status + ' ' : '') + String(message || '请求失败').slice(0, 400)
}

function chatgptApiCompactObject(value) {
  const result = {}
  for (const [key, item] of Object.entries(value || {})) {
    if (item !== '' && item !== null && item !== undefined) result[key] = item
  }
  return result
}

const CHATGPT_API_ORIGIN = 'https://chatgpt.com'
const CHATGPT_API_AUTH_FILE = 'auth.json'

function chatgptApiReadAuth(ctx) {
  const stored = host.call('file.read_json', {
    path: ctx.profile_dir + '/' + CHATGPT_API_AUTH_FILE,
  })
  if (!stored?.auth || !Array.isArray(stored?.cookies)) {
    throw new Error('LOGIN_EXPIRED: auth.json 缺少登录凭据，请重新登录 ChatGPT。')
  }
  Object.defineProperty(stored, 'storage_path', {
    value: ctx.profile_dir + '/' + CHATGPT_API_AUTH_FILE,
  })
  chatgptApiAlignFingerprint(stored)
  return stored
}

function chatgptApiSessionId() {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (char) => {
    const value = Math.floor(Math.random() * 16)
    return (char === 'x' ? value : (value & 3) | 8).toString(16)
  })
}

function chatgptApiRefreshSession(auth) {
  const key = 'chatgpt-auth:' + auth.storage_path
  host.call('lock.acquire', { key })
  try {
    if (auth.storage_path) {
      const latest = host.call('file.read_json', { path: auth.storage_path })
      auth.auth = latest.auth
      auth.cookies = latest.cookies
    }
    chatgptApiAlignFingerprint(auth)
    auth.refreshing = true
    if (!chatgptApiCookieHeader(auth))
      throw new Error('LOGIN_EXPIRED: 登录 Cookie 已过期，请重新登录 ChatGPT。')
    auth.runtime_started_at = Date.now()
    chatgptApiNavigateHome(auth, chatgptApiCookieHeader(auth))
    const result = chatgptApiFetchSession(auth, chatgptApiCookieHeader(auth, '/api/auth/session'))
    chatgptApiValidateSession(result)
    chatgptApiRememberCookies(auth, result, '/api/auth/session')
    chatgptApiApplySession(auth, result.body)
    chatgptApiSaveAuth(auth)
    return auth
  } finally {
    auth.refreshing = false
    host.call('lock.release', { key })
  }
}

function chatgptApiFetchSession(auth, cookie) {
  return host.call('http.request', {
    method: 'GET',
    url: CHATGPT_API_ORIGIN + '/api/auth/session',
    proxy: 'system',
    tls_profile: chatgptApiTlsProfile(auth),
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
    tls_profile: chatgptApiTlsProfile(auth),
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
  chatgptApiRememberCookies(auth, result)
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
  const status = Number(result.status)
  if (status === 401) {
    throw new Error('LOGIN_EXPIRED: ChatGPT 登录已失效，请重新登录。')
  }
  // 403 也可能来自设备校验或边缘风控，不能据此把真实登录账号永久标成过期。
  if (status === 403) {
    throw new Error('AUTH_INCONCLUSIVE: ChatGPT 暂时拒绝会话检查：' + chatgptApiHttpSummary(result))
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
    'chatgpt-account-id': chatgptApiAccountId(auth),
    cookie: chatgptApiCookieHeader(auth, targetPath),
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
    tls_profile: chatgptApiTlsProfile(auth),
    headers,
    timeout_ms: config.timeout_ms || (config.sse ? 3 * 60 * 60 * 1000 : 30000),
  }
  if (config.body !== undefined) request.body = config.body
  if (config.body_format) request.body_format = config.body_format
  if (config.body_format === 'multipart') delete request.headers['content-type']
  if (config.response_body_format) request.response_body_format = config.response_body_format
  if (config.retry) request.retry = config.retry
  if (config.sse && typeof config.on_event === 'function' && typeof host.stream === 'function') {
    const events = []
    const result = chatgptApiStreamWithResume(auth, request, (event) => {
      config.on_event(event)
      events.push(event)
      return null
    })
    chatgptApiRememberCookies(auth, result, path)
    return { ...result, events }
  }
  return chatgptApiRememberCookies(
    auth,
    host.call(config.sse ? 'http.sse_request' : 'http.request', request),
    path
  )
}

// 新网页会将生成转交给 Conduit。只接续原会话，不重新提交用户消息。
function chatgptApiStreamWithResume(auth, request, onEvent) {
  const deadline = Date.now() + (request.timeout_ms || 3 * 60 * 60 * 1000)
  let next = request
  let result
  let token = ''
  let conversationId = ''
  do {
    let handoff = false
    let completed = false
    result = host.stream('http.sse_request', next, (event) => {
      if (event?.type === 'resume_conversation_token') {
        token = typeof event.token === 'string' ? event.token : ''
        conversationId = String(event.conversation_id || '')
      }
      if (event?.type === 'stream_handoff') handoff = true
      if (event?.type === 'message_stream_complete') completed = true
      onEvent(event)
      if (event?.type === 'resume_conversation_token' && token && conversationId) {
        chatgptApiCheckpoint({
          ...chatgptApiCheckpoint(),
          phase: 'accepted',
          conversation_id: conversationId,
          resume_token: token,
        })
      }
      return null
    })
    if (!result.ok || !handoff || completed) return result
    if (!conversationId || !token) {
      throw new Error('PROVIDER_SUBMISSION_UNKNOWN: ChatGPT 转交结果缺少恢复凭据，请恢复原任务')
    }
    const remaining = deadline - Date.now()
    if (remaining <= 0) throw new Error('temporary_unavailable: ChatGPT 接续结果超时')
    next = chatgptApiResumeRequest(auth, conversationId, token, remaining)
  } while (true)
}

function chatgptApiResumeRequest(auth, conversationId, token, timeoutMs) {
  const path = '/backend-api/f/conversation/resume'
  return {
    method: 'POST',
    url: CHATGPT_API_ORIGIN + path,
    proxy: 'system',
    tls_profile: chatgptApiTlsProfile(auth),
    headers: {
      ...chatgptApiBaseHeaders(auth, path, path),
      accept: 'text/event-stream',
      'content-type': 'application/json',
      'x-conduit-token': token,
    },
    body: { conversation_id: conversationId, offset: 0 },
    retry: { max_attempts: 1 },
    timeout_ms: timeoutMs,
  }
}

function chatgptApiRequireOk(result, operation) {
  if (result?.ok) return result
  const status = Number(result?.status || 0)
  if (status === 401) {
    throw new Error('LOGIN_EXPIRED: ' + operation + ' 返回 HTTP ' + status)
  }
  if (status === 403) {
    throw new Error(
      'AUTH_INCONCLUSIVE: ' + operation + ' 暂时被拒绝：' + chatgptApiHttpSummary(result)
    )
  }
  const summary = chatgptApiHttpSummary(result)
  if (chatgptApiQuotaLikeResult(result, summary)) {
    throw new Error('quota_exhausted: ' + operation + ' 失败：' + summary)
  }
  if (status === 429) {
    throw new Error('rate_limit_exceeded: ' + operation + ' 失败：' + summary)
  }
  throw new Error(operation + ' 失败：' + summary)
}

function chatgptApiHttpSummary(result) {
  const status = Number(result?.status || 0)
  const body = result?.body
  const message =
    body?.detail || body?.error?.message || body?.error || body?.message || result?.body_text || ''
  const cooldownSeconds = chatgptApiCooldownSeconds(result)
  return (
    (status ? 'HTTP ' + status + ' ' : '') +
    String(message || '请求失败').slice(0, 400) +
    (cooldownSeconds ? '; retry after ' + cooldownSeconds + ' seconds' : '')
  )
}

function chatgptApiQuotaLikeResult(result, summary) {
  const body = result?.body
  const text =
    String(summary || '') +
    ' ' +
    (body && typeof body === 'object'
      ? JSON.stringify(body)
      : String(result?.body_text || body || ''))
  return /quota|insufficient[_\s-]?quota|usage limit|free plan limit|limit resets in|image generations requests|session\s*budget|sessionbudgetexceeded|limit exceeded/i.test(
    text
  )
}

function chatgptApiCooldownSeconds(result) {
  return (
    chatgptApiCooldownSecondsFromValue(result?.body) ||
    chatgptApiCooldownSecondsFromText(result?.body_text) ||
    chatgptApiCooldownSecondsFromValue(result?.headers) ||
    chatgptApiRetryAfterSeconds(result?.headers)
  )
}

function chatgptApiCooldownSecondsFromValue(value) {
  const seconds = []
  const visit = (item, key, depth) => {
    if (depth > 20 || item === null || item === undefined) return
    const normalizedKey = String(key || '')
      .trim()
      .toLowerCase()
      .replace(/[-\s]+/g, '_')
    if (
      normalizedKey === 'cooldown_seconds' ||
      normalizedKey === 'retry_after_seconds' ||
      normalizedKey === 'reset_after_seconds' ||
      normalizedKey === 'rate_limit_reset_after_seconds' ||
      normalizedKey.endsWith('_reset_after_seconds')
    ) {
      const value = chatgptApiPositiveSeconds(item)
      if (value) seconds.push(value)
    }
    if (
      ['reset_at', 'resets_at', 'rate_limit_reset_at', 'cooldown_until'].includes(normalizedKey)
    ) {
      const value = chatgptApiSecondsUntil(item)
      if (value) seconds.push(value)
    }
    if (typeof item === 'string') {
      const value = chatgptApiCooldownSecondsFromText(item)
      if (value) seconds.push(value)
    }
    if (Array.isArray(item)) {
      for (const child of item) visit(child, '', depth + 1)
    } else if (item && typeof item === 'object') {
      for (const [childKey, childValue] of Object.entries(item))
        visit(childValue, childKey, depth + 1)
    }
  }
  visit(value, '', 0)
  return seconds.length ? Math.max(...seconds) : 0
}

function chatgptApiCooldownSecondsFromText(text) {
  const unitSeconds = {
    second: 1,
    seconds: 1,
    sec: 1,
    secs: 1,
    秒: 1,
    minute: 60,
    minutes: 60,
    min: 60,
    mins: 60,
    分钟: 60,
    hour: 3600,
    hours: 3600,
    hr: 3600,
    hrs: 3600,
    小时: 3600,
    day: 86400,
    days: 86400,
    天: 86400,
  }
  const pattern = /(\d+)\s*(seconds?|secs?|minutes?|mins?|hours?|hrs?|days?|秒|分钟|小时|天)/gi
  let total = 0
  for (const match of String(text || '')
    .toLowerCase()
    .matchAll(pattern)) {
    total += Number(match[1] || 0) * Number(unitSeconds[match[2]] || 0)
  }
  return total > 0 ? Math.ceil(total) : 0
}

function chatgptApiRetryAfterSeconds(headers) {
  const raw = chatgptApiHeaderValue(headers, 'retry-after')
  if (!raw) return 0
  const seconds = chatgptApiPositiveSeconds(raw)
  if (seconds) return seconds
  return chatgptApiSecondsUntil(raw)
}

function chatgptApiHeaderValue(headers, name) {
  const expected = String(name || '').toLowerCase()
  for (const [key, value] of Object.entries(headers || {})) {
    if (String(key).toLowerCase() === expected) return String(value || '').trim()
  }
  return ''
}

function chatgptApiPositiveSeconds(value) {
  const seconds = Number(value)
  return Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : 0
}

function chatgptApiSecondsUntil(value) {
  const raw = String(value || '').trim()
  const numeric = Number(raw)
  let timestamp = 0
  if (Number.isFinite(numeric) && numeric > 0) {
    timestamp = numeric > 10000000000 ? numeric : numeric * 1000
  } else {
    timestamp = Date.parse(raw)
  }
  const seconds = (timestamp - Date.now()) / 1000
  return Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : 0
}

function chatgptApiCompactObject(value) {
  const result = {}
  for (const [key, item] of Object.entries(value || {})) {
    if (item !== '' && item !== null && item !== undefined) result[key] = item
  }
  return result
}

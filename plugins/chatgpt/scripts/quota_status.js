function chatgptApiImageQuotaStatus(auth) {
  const bootstrapMessage = chatgptApiQuotaMessageFromBootstrap(auth)
  const pageMessage = bootstrapMessage ? '' : chatgptApiQuotaMessageFromImagesPage(auth)
  const reason = bootstrapMessage || pageMessage
  if (!reason) return null
  const seconds = chatgptApiDurationSeconds(reason)
  return chatgptApiCompactObject({
    status: 'daily_quota_exhausted',
    reason,
    cooldown_seconds: seconds > 0 ? seconds : null,
  })
}

function chatgptApiQuotaMessageFromBootstrap(auth) {
  try {
    return chatgptApiQuotaMessageFromValue(
      chatgptApiBootstrap(auth)?.['/backend-api/images/bootstrap']
    )
  } catch (error) {
    // 配额接口允许临时失败，但明确的 401 必须交给账号检查标记为登录过期。
    chatgptApiRethrowLoginExpired(error)
    return ''
  }
}

function chatgptApiRethrowLoginExpired(error) {
  const message = String(error?.message || error || '')
  if (message.startsWith('LOGIN_EXPIRED:')) throw error
}

function chatgptApiQuotaMessageFromImagesPage(auth) {
  try {
    const response = host.call('http.request', {
      method: 'GET',
      url: CHATGPT_API_ORIGIN + '/images',
      proxy: 'system',
      tls_profile: chatgptApiTlsProfile(auth),
      headers: {
        ...chatgptApiBrowserHeaders(auth, 'none'),
        accept: 'text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8',
        cookie: chatgptApiCookieHeader(auth),
        'sec-fetch-dest': 'document',
        'sec-fetch-mode': 'navigate',
        'sec-fetch-site': 'none',
        'upgrade-insecure-requests': '1',
      },
      timeout_ms: 60000,
    })
    return response?.ok ? chatgptApiQuotaMessageFromText(String(response.body_text || '')) : ''
  } catch (_) {
    return ''
  }
}

function chatgptApiQuotaMessageFromValue(value) {
  return chatgptApiQuotaMessageFromText(chatgptApiQuotaTextCorpus(value))
}

function chatgptApiQuotaTextCorpus(value) {
  const parts = []
  chatgptApiQuotaWalk(value, (item) => {
    if (typeof item === 'string') parts.push(item)
  })
  return parts.join('\n')
}

function chatgptApiQuotaWalk(value, visit, depth) {
  const level = Number(depth || 0)
  if (level > 20 || value === null || value === undefined) return
  visit(value)
  if (Array.isArray(value)) {
    for (const item of value) chatgptApiQuotaWalk(item, visit, level + 1)
  } else if (typeof value === 'object') {
    for (const item of Object.values(value)) chatgptApiQuotaWalk(item, visit, level + 1)
  }
}

function chatgptApiQuotaMessageFromText(text) {
  const normalized = chatgptApiNormalizeQuotaText(text)
  const patterns = [
    /you(?:'|’)ve hit the [^.。]*limit for image generations requests[^.。]*(?:limit resets in|try again in)[^.。]*/i,
    /you(?:'|’)re out of image generation messages[^.。]*(?:please try again in|try again in)[^.。]*/i,
    /image generations requests[^.。]*(?:limit resets in|try again in)[^.。]*/i,
    /limit resets in\s+\d+\s*(?:seconds?|secs?|minutes?|mins?|hours?|hrs?|days?|秒|分钟|小时|天)(?:\s+and\s+\d+\s*(?:seconds?|secs?|minutes?|mins?|hours?|hrs?|days?|秒|分钟|小时|天))?/i,
  ]
  return patterns.map((pattern) => normalized.match(pattern)?.[0]).find(Boolean) || ''
}

function chatgptApiNormalizeQuotaText(text) {
  return String(text || '')
    .replace(/\\u([0-9a-f]{4})/gi, (_, code) => String.fromCharCode(parseInt(code, 16)))
    .replace(/<[^>]+>/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

function chatgptApiDurationSeconds(text) {
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
  return total
}

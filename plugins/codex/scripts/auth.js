const CODEX_CLIENT_ID = 'app_EMoamEEZ73f0CkXaXp7hrann'
const CODEX_TOKEN_URL = 'https://auth.openai.com/oauth/token'
const CODEX_ACCOUNT_CHECK_URL = 'https://chatgpt.com/backend-api/wham/accounts/check'
const CODEX_TOKEN_REFRESH_SKEW_SECONDS = 300

function codexAuthPath(ctx) {
  return ctx.profile_dir + '/auth.json'
}

function codexReadAuth(ctx) {
  return host.call('file.read_json', { path: codexAuthPath(ctx) })
}

function codexAccessToken(auth) {
  return codexGet(auth, ['tokens', 'access_token'])
}

function codexRefreshToken(auth, ctx) {
  return codexFirstNonEmpty([
    codexGet(auth, ['tokens', 'refresh_token']),
    codexGet(ctx, ['account', 'refresh_token']),
  ])
}

function codexEnsureFreshAuth(ctx) {
  const auth = codexReadAuth(ctx)
  if (!codexAccessToken(auth)) {
    throw new Error('LOGIN_EXPIRED: auth.json 缺少 access_token，请重新登录 codex。')
  }
  // 生成或健康检查前先刷新临近过期的 token，避免请求刚发出去就被 401 打断。
  if (!codexAccessTokenExpiresSoon(auth)) return auth
  return codexRefreshAuth(ctx, auth, 'access_token 即将过期')
}

function codexRefreshAuth(ctx, auth, reason) {
  const refreshToken = codexRefreshToken(auth, ctx)
  if (!refreshToken) {
    throw new Error('LOGIN_EXPIRED: 缺少 refresh_token，请重新登录 codex。')
  }
  const result = host.tryCall('http.request', {
    method: 'POST',
    url: CODEX_TOKEN_URL,
    proxy: 'system',
    body_format: 'form',
    body: {
      grant_type: 'refresh_token',
      refresh_token: refreshToken,
      client_id: CODEX_CLIENT_ID,
    },
    timeout_ms: 30000,
  })
  if (!result.success) {
    throw new Error(
      'LOGIN_EXPIRED: token 刷新失败（' + reason + '，' + codexHttpSummary(result) + '）'
    )
  }
  const token = result.data.body || {}
  if (!codexString(token.access_token)) {
    throw new Error('LOGIN_EXPIRED: token 刷新响应缺少 access_token，请重新登录 codex。')
  }
  const next = codexBuildAuthFile(token, auth)
  host.call('file.write_json', { path: codexAuthPath(ctx), value: next })
  return next
}

function codexFetchAccountProfile(auth) {
  const headers = {
    authorization: 'Bearer ' + codexAccessToken(auth),
    accept: 'application/json',
  }
  const accountId = codexAccountId(auth)
  if (accountId) headers['chatgpt-account-id'] = accountId
  return host.tryCall('http.request', {
    method: 'GET',
    url: CODEX_ACCOUNT_CHECK_URL,
    proxy: 'system',
    headers,
    timeout_ms: 30000,
  })
}

function codexAccountPatch(auth, profile) {
  const idPayload = codexDecodeJwtPayload(codexGet(auth, ['tokens', 'id_token']))
  const accessPayload = codexDecodeJwtPayload(codexAccessToken(auth))
  const email = codexFirstNonEmpty([
    codexFindString(profile, ['email', 'account_email']),
    codexGet(idPayload, ['email']),
    codexGet(accessPayload, ['email']),
  ])
  const userId = codexFirstNonEmpty([
    codexFindString(profile, ['user_id', 'id', 'sub']),
    codexGet(idPayload, ['sub']),
    codexGet(accessPayload, ['sub']),
    codexAccountId(auth),
  ])
  const username = codexFirstNonEmpty([
    codexFindString(profile, ['name', 'username', 'display_name']),
    email,
    codexAccountId(auth),
    'codex',
  ])
  return {
    api_token: codexAccessToken(auth),
    refresh_token: codexRefreshToken(auth, {}),
    email,
    user_id: userId,
    username,
    name: username,
  }
}

function codexBuildAuthFile(token, previousAuth) {
  const accessToken = codexFirstNonEmpty([
    codexString(token.access_token),
    codexGet(previousAuth, ['tokens', 'access_token']),
  ])
  const idToken = codexFirstNonEmpty([
    codexString(token.id_token),
    codexGet(previousAuth, ['tokens', 'id_token']),
  ])
  const refreshToken = codexFirstNonEmpty([
    codexString(token.refresh_token),
    codexGet(previousAuth, ['tokens', 'refresh_token']),
  ])
  const accountId = codexFirstNonEmpty([
    codexGet(previousAuth, ['tokens', 'account_id']),
    codexAccountIdFromToken(accessToken),
    codexAccountIdFromToken(idToken),
  ])
  return {
    OPENAI_API_KEY: null,
    tokens: {
      id_token: idToken,
      access_token: accessToken,
      refresh_token: refreshToken,
      account_id: accountId,
    },
    last_refresh: new Date().toISOString(),
  }
}

function codexAccessTokenExpiresSoon(auth) {
  const exp = Number(codexPath(codexDecodeJwtPayload(codexAccessToken(auth)), ['exp']))
  if (!Number.isFinite(exp) || exp <= 0) return false
  return exp <= Math.floor(Date.now() / 1000) + CODEX_TOKEN_REFRESH_SKEW_SECONDS
}

function codexErrorNeedsTokenRefresh(error) {
  const text = String((error && error.message) || error || '').toLowerCase()
  return (
    text.includes('http 401') || text.includes('invalid_token') || text.includes('unauthorized')
  )
}

function codexHealthFromHttpResult(result, auth) {
  const status = codexHttpStatus(result)
  if (result.success) {
    return {
      logged_in: true,
      status: 'available',
      reason: '账号健康检查通过',
      account: codexAccountPatch(auth, codexPath(result, ['data', 'body'])),
    }
  }
  if (status === 401) {
    return { logged_in: false, status: 'login_expired', reason: codexHttpSummary(result) }
  }
  if ([403, 429, 500, 502, 503, 504].indexOf(status) >= 0) {
    return { logged_in: null, status: 'inconclusive', reason: codexHttpSummary(result) }
  }
  return { logged_in: false, status: 'login_expired', reason: codexHttpSummary(result) }
}

function codexAccountId(auth) {
  return codexFirstNonEmpty([
    codexGet(auth, ['tokens', 'account_id']),
    codexAccountIdFromToken(codexAccessToken(auth)),
    codexAccountIdFromToken(codexGet(auth, ['tokens', 'id_token'])),
  ])
}

function codexAccountIdFromToken(token) {
  const payload = codexDecodeJwtPayload(token)
  return codexFirstNonEmpty([
    codexGet(payload, ['https://api.openai.com/auth', 'account_id']),
    codexGet(payload, ['https://api.openai.com/auth', 'chatgpt_account_id']),
  ])
}

function codexDecodeJwtPayload(token) {
  const parts = String(token || '').split('.')
  if (parts.length < 2) return {}
  try {
    return JSON.parse(host.call('crypto.base64url_decode_utf8', { text: parts[1] }).value)
  } catch (_) {
    return {}
  }
}

function codexHttpSummary(result) {
  const status = codexHttpStatus(result)
  const error = codexString(result.error)
  return error || (status ? 'HTTP ' + status : '请求失败')
}

function codexHttpStatus(result) {
  return Number(codexPath(result, ['data', 'status'])) || 0
}

function codexFindString(value, keys) {
  if (!value || typeof value !== 'object') return ''
  for (const key of keys) {
    const direct = codexString(value[key])
    if (direct) return direct
  }
  for (const key in value) {
    const nested = codexFindString(value[key], keys)
    if (nested) return nested
  }
  return ''
}

function codexGet(value, path) {
  return codexString(codexPath(value, path))
}

function codexPath(value, path) {
  let current = value
  for (const key of path) {
    if (!current || typeof current !== 'object') return undefined
    current = current[key]
  }
  return current
}

function codexString(value) {
  return typeof value === 'string' ? value.trim() : ''
}

function codexFirstNonEmpty(values) {
  for (const value of values) {
    const text = codexString(value)
    if (text) return text
  }
  return ''
}

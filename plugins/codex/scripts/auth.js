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
  const key = 'credential:' + codexAuthPath(ctx)
  host.call('lock.acquire', { key })
  try {
    const latest = codexReadAuth(ctx)
    if (
      codexAccessToken(latest) &&
      codexAccessToken(latest) !== codexAccessToken(auth) &&
      !codexAccessTokenExpiresSoon(latest)
    )
      return latest
    return codexRefreshAuthLocked(ctx, latest, reason)
  } finally {
    host.call('lock.release', { key })
  }
}

function codexRefreshAuthLocked(ctx, auth, reason) {
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
  if (!codexHttpSucceeded(result)) {
    const kind = codexRefreshErrorKind(result)
    const prefix = codexRefreshFailureRequiresLogin(kind) ? 'LOGIN_EXPIRED' : 'AUTH_INCONCLUSIVE'
    throw new Error(
      prefix + ': token 刷新失败（' + reason + '）：' + codexRefreshFailureMessage(result, kind)
    )
  }
  const token = result.data.body || {}
  if (!codexString(token.access_token)) {
    throw new Error('AUTH_INCONCLUSIVE: token 刷新响应缺少 access_token。')
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
  if (codexHttpSucceeded(result)) {
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
  return { logged_in: null, status: 'inconclusive', reason: codexHttpSummary(result) }
}

function codexHttpSucceeded(result) {
  return result.success === true && result.data?.ok === true
}

function codexRefreshErrorKind(result) {
  const text = codexHttpSummary(result).toLowerCase()
  if (text.includes('unsupported_country_region_territory')) return 'unsupported_country_region'
  if (text.includes('refresh_token_reused')) return 'refresh_token_reused'
  if (text.includes('refresh_token_expired')) return 'refresh_token_expired'
  if (text.includes('refresh_token_invalidated')) return 'refresh_token_invalidated'
  if (text.includes('token_invalidated')) return 'refresh_token_invalidated'
  if (text.includes('authentication token has been invalidated')) return 'refresh_token_invalidated'
  if (text.includes('invalid_grant')) return 'invalid_grant'
  if (text.includes('invalid refresh token')) return 'invalid_grant'
  return 'other'
}

function codexRefreshFailureRequiresLogin(kind) {
  return [
    'refresh_token_reused',
    'refresh_token_expired',
    'refresh_token_invalidated',
    'invalid_grant',
  ].includes(kind)
}

function codexRefreshFailureMessage(result, kind) {
  const original = codexHttpSummary(result)
  const messages = {
    refresh_token_reused:
      'Codex 授权已失效：refresh_token 已被其它客户端或实例使用过。请重新登录，并避免官方 Codex、其它实例或外部工具同时刷新同一账号。',
    refresh_token_expired: 'Codex 登录授权已过期，无法自动刷新。请重新登录 Codex 账号。',
    refresh_token_invalidated:
      'Codex 登录授权已被服务端撤销，无法自动刷新。请重新登录 Codex 账号。',
    invalid_grant: 'Codex 登录授权无效，无法自动刷新。请重新登录 Codex 账号。',
    unsupported_country_region: '当前网络地区不支持刷新 Codex 授权。请切换到支持的网络地区后重试。',
  }
  return (messages[kind] || 'Codex token 刷新失败。') + ' 原始错误: ' + original
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
  const error = codexString(result.error)
  if (error) return error
  const body = codexBodySummary(codexPath(result, ['data', 'body']))
  if (body) return body
  const status = codexHttpStatus(result)
  return status ? 'HTTP ' + status : '请求失败'
}

function codexHttpStatus(result) {
  return Number(codexPath(result, ['data', 'status'])) || 0
}

function codexBodySummary(body) {
  if (!body || typeof body !== 'object') return codexString(body)
  const code = codexFirstNonEmpty([codexGet(body, ['error', 'code']), codexGet(body, ['code'])])
  const message = codexFirstNonEmpty([
    codexGet(body, ['error', 'message']),
    codexGet(body, ['message']),
  ])
  return [code, message].filter(Boolean).join(': ')
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
const CODEX_UPSTREAM_VERSION = '0.154.0'
const CODEX_UPSTREAM_USER_AGENT =
  'codex-tui/' + CODEX_UPSTREAM_VERSION + ' (Ubuntu 22.4.0; x86_64) xterm-256color'

function codexUpstreamHeaders(accessToken, accountId, accept) {
  const headers = {
    authorization: 'Bearer ' + accessToken,
    accept: accept || 'text/event-stream',
    'content-type': 'application/json',
    'openai-beta': 'responses=experimental',
    originator: 'codex-tui',
    version: CODEX_UPSTREAM_VERSION,
    'user-agent': CODEX_UPSTREAM_USER_AGENT,
    'session-id': globalThis.__context?.request_id || globalThis.__context?.task_id || '',
  }
  if (accountId) headers['chatgpt-account-id'] = accountId
  return headers
}

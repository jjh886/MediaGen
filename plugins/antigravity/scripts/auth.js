const MEDIAGEN_OAUTH_CLIENT = {
  client_id: '1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com',
  client_secret: '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',
}

const AG_ENDPOINT = 'https://cloudcode-pa.googleapis.com/v1internal:'

function agRequireOk(result, label) {
  if (result.ok) return result.body
  const status = result.status
  const category =
    result.body?.error === 'invalid_grant' || status === 401
      ? 'LOGIN_EXPIRED'
      : status === 403 || status === 404
        ? 'account_model_unavailable'
        : status === 429
          ? 'rate_limit'
          : 'upstream_error'
  const details = result.body?.error?.details
  const reason =
    (Array.isArray(details) &&
      details.find((item) => /^[A-Z_]{1,64}$/.test(item?.reason))?.reason) ||
    (/^[A-Z_]{1,64}$/.test(result.body?.error?.status) && result.body.error.status)
  const retry =
    Array.isArray(details) &&
    details.find((item) => /^\d+(\.\d+)?s$/.test(item?.retryDelay))?.retryDelay
  throw new Error(
    category +
      ': ' +
      label +
      ' HTTP ' +
      status +
      (reason ? ' ' + reason : '') +
      (retry ? ' retry after ' + retry : '')
  )
}

function agRegistration(ctx, expectedId) {
  let client = MEDIAGEN_OAUTH_CLIENT
  if (!client.client_id && !client.client_secret) {
    const saved = host.tryCall('file.read_json', { path: ctx.profile_dir + '/oauth-client.json' })
    if (!saved.success) throw new Error('oauth_client_unconfigured: 部署端缺少已授权的应用注册配置')
    client = saved.data
  }
  if (
    typeof client?.client_id !== 'string' ||
    !client.client_id.trim() ||
    typeof client.client_secret !== 'string' ||
    !client.client_secret.trim()
  )
    throw new Error('oauth_client_unconfigured: 请完整填写应用注册配置')
  if (expectedId && expectedId !== client.client_id)
    throw new Error('LOGIN_EXPIRED: 授权客户端不匹配，请重新登录')
  return { client_id: client.client_id, client_secret: client.client_secret }
}

function agExchange(ctx, grant, previous = {}) {
  const client = agRegistration(ctx, grant.client_id)
  const bodyParams = { ...grant, ...client }
  const result = host.call('http.request', {
    method: 'POST',
    url: 'https://oauth2.googleapis.com/token',
    body_format: 'form',
    body: bodyParams,
    timeout_ms: 30000,
    follow_redirects: false,
    retry: { max_attempts: 1 },
    proxy: 'system',
  })
  if (
    result.body?.error === 'invalid_client' ||
    /client_secret is missing/i.test(result.body?.error_description || '')
  )
    throw new Error('oauth_client_rejected: Google 拒绝了插件客户端的令牌交换，请检查应用注册配置')
  const body = agRequireOk(result, 'Antigravity OAuth')
  const refresh = body?.refresh_token || previous.refresh_token
  const lifetime = Number(body?.expires_in)
  if (!body?.access_token || !refresh || !Number.isFinite(lifetime) || lifetime <= 0)
    throw new Error('AUTH_INCONCLUSIVE: Antigravity 授权未返回可续期的令牌')
  if (
    body.scope &&
    !body.scope.split(/\s+/).includes('https://www.googleapis.com/auth/cloud-platform')
  )
    throw new Error('account_permission_denied: 本次授权未包含模型接口所需的 Google Cloud 权限')
  return {
    ...previous,
    provider: 'antigravity',
    client_id: client.client_id,
    access_token: body.access_token,
    refresh_token: refresh,
    expires_at: Date.now() + lifetime * 1000,
  }
}

function agAuth(ctx, rejectedToken, forceRefresh = false) {
  const path = ctx.profile_dir + '/auth.json'
  const key = 'credential:' + path
  host.call('lock.acquire', { key })
  try {
    let auth = host.call('file.read_json', { path })
    if (auth?.provider !== 'antigravity' || !auth.refresh_token || !auth.client_id)
      throw new Error('LOGIN_EXPIRED: 请先完成 Antigravity 账号登录')
    agRegistration(ctx, auth.client_id)
    function refreshToken() {
      auth = agExchange(
        ctx,
        {
          grant_type: 'refresh_token',
          client_id: auth.client_id,
          refresh_token: auth.refresh_token,
        },
        auth
      )
      host.call('file.write_json', { path, value: auth })
    }
    const needsRefresh =
      forceRefresh ||
      !(auth.expires_at > Date.now() + 50 * 60 * 1000) ||
      !auth.access_token ||
      auth.access_token === rejectedToken
    if (needsRefresh) refreshToken()
    if (
      forceRefresh ||
      !(auth.tier_checked_at > Date.now() - 3600000) ||
      auth.project_endpoint !== agModelEndpoint(auth)
    ) {
      let info
      try {
        info = agProjectInfo(auth)
      } catch (error) {
        if (needsRefresh || !String(error.message).includes('HTTP 401')) throw error
        refreshToken()
        info = agProjectInfo(auth)
      }
      auth.project = agProjectId(info) || auth.project
      host.call('file.write_json', { path, value: auth })
    }
    return auth
  } finally {
    host.call('lock.release', { key })
  }
}

function agParams(auth, url, body) {
  return {
    method: body === undefined ? 'GET' : 'POST',
    url,
    headers: {
      Authorization: 'Bearer ' + auth.access_token,
      'User-Agent': 'MediaGen-Antigravity/1.0.6',
      ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
    },
    body,
    follow_redirects: false,
    timeout_ms: 30000,
    retry: { max_attempts: 1 },
    proxy: 'system',
  }
}

function agRequest(auth, action, body) {
  return agRequireOk(host.call('http.request', agParams(auth, AG_ENDPOINT + action, body)), action)
}

function agLoadProject(auth) {
  const info = agProjectInfo(auth)
  const existing = agProjectId(info)
  if (existing) return existing
  const tier = info.allowedTiers?.find((item) => item.isDefault)?.id
  if (!tier) throw new Error('account_model_unavailable: 当前账号没有可用的 Antigravity 项目')
  for (let i = 0; i < 5; i++) {
    if (i) host.call('time.sleep', { milliseconds: 2000 })
    const result = agRequest(auth, 'onboardUser', {
      tierId: tier,
      metadata: { ideType: 'ANTIGRAVITY', platform: 'PLATFORM_UNSPECIFIED', pluginType: 'GEMINI' },
    })
    if (result.done && agProjectId(result.response)) return agProjectId(result.response)
    if (result.done) break
  }
  throw new Error('AUTH_INCONCLUSIVE: Google 尚未返回 Antigravity 项目')
}

function agProjectId(info) {
  const project = info?.cloudaicompanionProject
  const id = typeof project === 'string' ? project : project?.id
  return typeof id === 'string' ? id.trim() : ''
}

function agProjectInfo(auth) {
  const metadata = { metadata: { ideType: 'ANTIGRAVITY' } }
  const initial = agRequest(auth, 'loadCodeAssist', metadata)
  agRememberTier(auth, initial)
  const endpoint = agModelEndpoint(auth)
  let info = initial
  if (endpoint !== AG_ENDPOINT) {
    const paid = agRequireOk(
      host.call('http.request', agParams(auth, endpoint + 'loadCodeAssist', metadata)),
      '读取付费端点项目'
    )
    if (agProjectId(paid)) info = paid
  }
  auth.project_endpoint = endpoint
  return info
}

function agRememberTier(auth, info) {
  const tier = info?.paidTier
  auth.paid_tier = typeof tier === 'string' ? tier : String(tier?.id || '')
  const current = info?.currentTier
  const fallback = info?.allowedTiers?.find((item) => item?.isDefault)?.id
  const raw = auth.paid_tier || (typeof current === 'string' ? current : current?.id) || fallback
  if (auth.account && typeof raw === 'string') {
    const name = typeof tier?.name === 'string' ? tier.name.trim() : ''
    auth.account.tier = name
      ? name + (fallback ? '(' + fallback.replace('-tier', '') + ')' : '')
      : fallback || raw
  }
  auth.credits = Array.isArray(tier?.availableCredits)
    ? tier.availableCredits.flatMap((item) => {
        const rawAmount = item?.creditAmount
        if (typeof rawAmount !== 'number' && !(typeof rawAmount === 'string' && rawAmount.trim()))
          return []
        const amount = Number(rawAmount)
        if (!Number.isFinite(amount) || amount < 0) return []
        return [{ credit_type: String(item.creditType || 'AI Credits'), amount }]
      })
    : []
  auth.tier_checked_at = Date.now()
}

// Pro 和 Ultra 的模型目录与生成必须使用同一套餐端点。
function agModelEndpoint(auth) {
  return /^g1-(pro|ultra)(-[a-z0-9]+)*-tier$/.test(auth.paid_tier)
    ? 'https://daily-cloudcode-pa.googleapis.com/v1internal:'
    : AG_ENDPOINT
}

function agCatalog(ctx, auth) {
  const request = (current) =>
    host.call(
      'http.request',
      agParams(current, agModelEndpoint(current) + 'fetchAvailableModels', {
        project: current.project,
      })
    )
  let result = request(auth)
  if (result.status === 401) {
    auth = agAuth(ctx, auth.access_token)
    result = request(auth)
  }
  const models = agRequireOk(result, '获取账号模型目录')?.models
  if (!models || typeof models !== 'object' || Array.isArray(models))
    throw new Error('invalid_response: Google 返回了无效的模型目录')
  return { auth, models: agNormalizeCatalog(models) }
}

function agNormalizeCatalog(models) {
  const catalog = Object.create(null)
  for (const [id, model] of Object.entries(models)) {
    if (!model || typeof model !== 'object' || Array.isArray(model)) continue
    const slug = id
      .trim()
      .replace(/^models\//, '')
      .replace(/\([^()]+\)$/, '')
      .trim()
    if (!/^(gemini-|claude-|gpt-)/.test(slug)) continue
    const canonical = /^gemini-4-(neon|carbon|argon|hydrogen)$/.test(slug) ? slug + '-medium' : slug
    const key = canonical.replace(/^gemini-claude-/, 'claude-')
    if (!Object.hasOwn(catalog, key) || key === id || 'models/' + key === id) catalog[key] = model
  }
  return catalog
}

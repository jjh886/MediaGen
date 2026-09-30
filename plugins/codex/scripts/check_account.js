function main(ctx) {
  let auth
  try {
    auth = codexEnsureFreshAuth(ctx)
  } catch (error) {
    return codexHealthFromError(error)
  }

  let result = codexFetchAccountProfile(auth, ctx)
  if (!codexHttpSucceeded(result) && codexHttpStatus(result) === 401) {
    try {
      auth = codexRefreshAuth(ctx, auth, '账号检查返回 401')
      result = codexFetchAccountProfile(auth, ctx)
    } catch (error) {
      return codexHealthFromError(error)
    }
  }

  const health = codexHealthFromHttpResult(result, auth)
  if (health.status !== 'available') return health
  const usage = host.tryCall('http.request', {
    method: 'GET',
    url: 'https://chatgpt.com/backend-api/wham/usage',
    headers: codexUpstreamHeaders(
      codexAccessToken(auth),
      codexAccountId(auth),
      'application/json',
      ctx
    ),
    proxy: 'system',
    timeout_ms: 20000,
  })
  if (!codexHttpSucceeded(usage))
    return { ...health, status: 'inconclusive', reason: '登录有效，但额度暂时无法确认' }
  return { ...health, ...codexUsageStatus(usage.data.body) }
}

function codexUsageStatus(body) {
  const limit = body?.rate_limit
  if (!limit) return { status: 'inconclusive', reason: '额度接口未返回有效窗口' }
  const now = Date.now()
  const windows = []
  for (const [key, label] of [
    ['primary_window', '短周期'],
    ['secondary_window', '长周期'],
  ]) {
    const item = limit[key]
    if (!item || !Number.isFinite(item.used_percent)) continue
    const seconds = Number(item.limit_window_seconds)
    const absoluteReset = Number(item.reset_at) * 1000
    const relativeReset = Number(item.reset_after_seconds)
    const reset =
      Number.isFinite(absoluteReset) && absoluteReset > 0
        ? absoluteReset
        : Number.isFinite(relativeReset) && relativeReset > 0
          ? now + relativeReset * 1000
          : 0
    windows.push({
      name: seconds === 18000 ? '5 小时' : seconds === 604800 ? '每周' : label,
      task_kind: 'text',
      remaining_percent: Math.max(0, Math.min(100, 100 - item.used_percent)),
      reset_at: reset > 0 && reset < 8.64e15 ? new Date(reset).toISOString() : null,
    })
  }
  if (!windows.length) return { status: 'inconclusive', reason: '额度接口未返回有效窗口' }
  const blocked = windows.filter(
    (w) => w.remaining_percent === 0 && (!w.reset_at || Date.parse(w.reset_at) > now)
  )
  const limited = limit.allowed === false || limit.limit_reached === true || blocked.length > 0
  const resets = blocked
    .map((w) => Date.parse(w.reset_at))
    .filter((t) => Number.isFinite(t) && t > now)
  return {
    quota_windows: windows,
    quota_task_kind: 'text',
    status: limited ? 'daily_quota_exhausted' : 'available',
    cooldown_seconds:
      limited && resets.length ? Math.ceil((Math.max(...resets) - now) / 1000) : null,
    reason: limited ? '文本额度已用完，等待平台窗口恢复' : '登录与文本额度正常',
  }
}

function codexHealthFromError(error) {
  const message = String((error && error.message) || error || 'Codex 登录检查失败')
  if (message.startsWith('AUTH_INCONCLUSIVE:')) {
    return {
      logged_in: null,
      status: 'inconclusive',
      reason: message.replace(/^AUTH_INCONCLUSIVE:\s*/, ''),
    }
  }
  const expired = message.startsWith('LOGIN_EXPIRED:')
  return {
    logged_in: expired ? false : null,
    status: expired ? 'login_expired' : 'inconclusive',
    reason: message.replace(/^LOGIN_EXPIRED:\s*/, ''),
  }
}

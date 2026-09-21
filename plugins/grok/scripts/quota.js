// 仅用于展示订阅类型，不把未校验的 JWT 字段用于任何权限判断。
function grokSubscriptionTier(auth) {
  try {
    const payload = String(auth.access_token || '').split('.')[1]
    if (!payload) return ''
    const claims = JSON.parse(host.call('crypto.base64url_decode_utf8', { text: payload }).value)
    const tiers = [
      'free',
      'supergrok',
      'x_basic',
      'x_premium',
      'x_premium_plus',
      'supergrok_heavy',
      'supergrok_lite',
      'supergrok_plus',
    ]
    const index = claims.tier
    return typeof index === 'number' && Number.isInteger(index) ? tiers[index] || '' : ''
  } catch (_) {
    return ''
  }
}
function grokQuotaNumber(value) {
  if (value && typeof value === 'object') value = value.val
  if (value === null || value === undefined || value === '') return null
  const number = Number(value)
  return Number.isFinite(number) ? number : null
}
function grokQuotaDate(value) {
  if (!value) return null
  const timestamp =
    typeof value === 'number' ? (value < 1e12 ? value * 1000 : value) : Date.parse(value)
  return Number.isFinite(timestamp) ? new Date(timestamp).toISOString() : null
}
function grokBillingWindows(weekly, monthly) {
  const windows = []
  const add = (name, used, reset) => {
    if (used !== null)
      windows.push({
        name,
        task_kind: 'text',
        remaining_percent: Math.max(0, Math.min(100, 100 - used)),
        reset_at: grokQuotaDate(reset),
      })
  }
  const week = weekly?.config
  const month = monthly?.config
  if (week) {
    add('每周额度', grokQuotaNumber(week.creditUsagePercent), week.currentPeriod?.end)
    for (const product of week.productUsage || []) {
      if (product.product)
        add(
          '每周 · ' + product.product,
          grokQuotaNumber(product.usagePercent),
          week.currentPeriod?.end
        )
    }
  }
  const limit = grokQuotaNumber(month?.monthlyLimit)
  const used = grokQuotaNumber(month?.used)
  if (limit > 0 && used !== null)
    add('每月额度', (used / limit) * 100, month.billingPeriodEnd || month.currentPeriod?.end)
  return windows
}
function grokHeaderWindows(headers) {
  const lower = Object.fromEntries(
    Object.entries(headers || {}).map(([key, value]) => [key.toLowerCase(), value])
  )
  const windows = []
  for (const [kind, name] of [
    ['requests', '请求速率额度'],
    ['tokens', 'Token 速率额度'],
  ]) {
    const get = (field) =>
      lower['x-ratelimit-' + field + '-' + kind] ?? lower['x-rate-limit-' + field + '-' + kind]
    const remaining = grokQuotaNumber(get('remaining'))
    const limit = grokQuotaNumber(get('limit'))
    if (remaining === null) continue
    let reset = get('reset')
    if (/^\d+(\.\d+)?$/.test(String(reset))) {
      const n = Number(reset)
      reset = n >= 1e12 ? n : n < 1e9 ? Date.now() + n * 1000 : n * 1000
    } else if (/^\d+(\.\d+)?(ms|s|m|h)$/.test(String(reset))) {
      reset =
        Date.now() +
        parseFloat(reset) *
          (String(reset).endsWith('ms')
            ? 1
            : String(reset).endsWith('s')
              ? 1000
              : String(reset).endsWith('m')
                ? 60000
                : 3600000)
    }
    windows.push({
      name,
      task_kind: 'text',
      remaining_count: remaining,
      unit: kind === 'tokens' ? 'Token' : '次',
      remaining_percent: limit > 0 ? Math.max(0, Math.min(100, (remaining / limit) * 100)) : null,
      reset_at: grokQuotaDate(reset),
    })
  }
  return windows
}
function grokQuota(ctx, auth) {
  const get = (path, body) => {
    try {
      return grokRequest(ctx, auth, path, body, {
        build: true,
        timeout_ms: 20000,
        headers: {
          ...grokHeaders(ctx, auth, true),
          'user-agent': 'xai-grok-workspace/' + GROK_VERSION,
          'x-grok-client-mode': 'cli',
        },
      })
    } catch (error) {
      if (String(error.message).startsWith('LOGIN_EXPIRED:')) throw error
      return null
    }
  }
  const weekly = get('/billing?format=credits')
  const monthly = get('/billing')
  const windows = grokBillingWindows(weekly?.body, monthly?.body)
  let headerWindows = grokHeaderWindows(weekly?.headers || monthly?.headers)
  if (!windows.length && !headerWindows.length && ctx.refresh_quota) {
    const probe = get('/responses', {
      model: 'grok-4.5',
      input: 'Reply OK.',
      stream: false,
      max_output_tokens: 8,
    })
    headerWindows = grokHeaderWindows(probe?.headers)
  }
  windows.push(...headerWindows)
  const limit = grokQuotaNumber(monthly?.body?.config?.monthlyLimit)
  const tier =
    grokSubscriptionTier(auth) ||
    (limit >= 150000 ? 'supergrok_heavy' : limit >= 15000 ? 'supergrok' : '')
  return { windows, tier, confirmed: !!(weekly || monthly), partial: !weekly || !monthly }
}

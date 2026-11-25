function main(ctx) {
  try {
    const { auth, models } = agCatalog(ctx, agAuth(ctx, undefined, ctx.refresh_quota === true))
    const summary = host.tryCall(
      'http.request',
      agParams(auth, agModelEndpoint(auth) + 'retrieveUserQuotaSummary', { project: auth.project })
    )
    const groups = summary.success && summary.data?.ok ? summary.data.body?.groups : null
    const windows = agQuotaWindows(models, groups, auth.credits)
    if (ctx.refresh_quota && !windows.length)
      return { status: 'inconclusive', reason: '上游本次未提供可确认的额度数据' }
    return {
      logged_in: true,
      status: 'available',
      account: auth.account,
      ...(windows.length ? { quota_windows: windows, quota_task_kind: 'text' } : {}),
    }
  } catch (error) {
    const message = String(error.message)
    if (message.includes('LOGIN_EXPIRED:'))
      return { logged_in: false, status: 'login_expired', reason: 'Google 授权已失效' }
    return { status: 'inconclusive', reason: '暂时无法确认 Antigravity 账号状态' }
  }
}

function agQuotaWindows(models, groups, credits) {
  const windows = []
  function addWindow(name, quota) {
    if (!name || !quota || typeof quota !== 'object' || Array.isArray(quota)) return
    const fraction = quota.remainingFraction ?? quota.remaining ?? 0
    if (!Number.isFinite(fraction) || fraction < 0 || fraction > 1) return
    const reset = Date.parse(quota.resetTime)
    windows.push({
      name,
      task_kind: 'text',
      remaining_percent: fraction * 100,
      ...(Number.isFinite(reset) ? { reset_at: new Date(reset).toISOString() } : {}),
    })
  }
  for (const group of Array.isArray(groups) ? groups : []) {
    if (!Array.isArray(group?.buckets)) continue
    const order = { '5h': 0, weekly: 1 }
    for (const bucket of [...group.buckets].sort(
      (a, b) => (order[a?.window] ?? 2) - (order[b?.window] ?? 2)
    )) {
      if (!bucket) continue
      const name = [group.displayName, bucket.window || bucket.displayName]
        .filter(Boolean)
        .join(' · ')
      addWindow(name || bucket.bucketId, bucket)
    }
  }
  const groupCount = windows.length
  for (const id of Object.keys(models).sort()) addWindow(id, models[id].quotaInfo)
  const perModel = windows.slice(groupCount)
  if (perModel.length) {
    const sum = perModel.reduce((total, row) => total + row.remaining_percent, 0)
    const resetAt = perModel
      .map((row) => row.reset_at)
      .filter(Boolean)
      .sort()
      .at(-1)
    windows.unshift({
      name: '模型平均剩余额度',
      task_kind: 'text',
      remaining_percent: sum / perModel.length,
      ...(resetAt ? { reset_at: resetAt } : {}),
    })
  }
  for (const credit of credits || []) {
    windows.push({ name: credit.credit_type, remaining_count: credit.amount, unit: '积分' })
  }
  return windows
}

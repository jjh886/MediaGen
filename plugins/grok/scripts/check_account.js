function main(ctx) {
  try {
    const auth = grokAuth(ctx)
    const quota = grokQuota(ctx, auth)
    if (!quota.confirmed) grokRequest(ctx, auth, '/models', undefined, { build: true })
    const result = {
      status: 'available',
      logged_in: true,
      quota_task_kind: 'text',
      account: {
        user_id: auth.user_id,
        email: auth.email || '',
        username: auth.email || auth.user_id,
        tier: quota.tier,
      },
      reason: quota.windows.length ? '登录与额度已刷新' : '登录有效，上游未返回可显示额度',
    }
    if (quota.windows.length) result.quota_windows = quota.windows
    else if (ctx.refresh_quota)
      return { ...result, status: 'inconclusive', reason: '额度暂时无法确认，请稍后重试' }
    const exhausted = quota.windows.filter(
      (w) => w.remaining_count === 0 && w.reset_at && Date.parse(w.reset_at) > Date.now()
    )
    if (exhausted.length) {
      result.status = 'daily_quota_exhausted'
      result.cooldown_seconds = Math.ceil(
        (Math.max(...exhausted.map((w) => Date.parse(w.reset_at))) - Date.now()) / 1000
      )
    }
    return result
  } catch (error) {
    const expired = String(error.message).startsWith('LOGIN_EXPIRED:')
    return {
      status: expired ? 'login_expired' : 'inconclusive',
      logged_in: expired ? false : null,
      reason: expired ? 'Grok 登录已过期' : 'Grok 登录/额度检查暂时失败',
    }
  }
}

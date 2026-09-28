function main(ctx) {
  try {
    const session = apiSessionRead(ctx)
    const user = doubaoRequest(session, '/passport/account/info/v2/', undefined, 'GET')
    if (!user.user_id && !user.user_id_str) throw new Error('LOGIN_EXPIRED: 豆包身份已失效')
    const overview = doubaoRequest(session, '/alice/commerce/sale/subscription/overview/', {})
    const quota = doubaoQuotaWindows(overview)
    return {
      logged_in: true,
      status: session.auth.verification_required
        ? 'verification_required'
        : overview.window_limit_section?.usage_exhausted
          ? 'daily_quota_exhausted'
          : 'available',
      user_id: String(user.user_id_str || user.user_id),
      username: user.screen_name || user.name || user.mobile || user.email || '',
      name: user.screen_name || user.name || user.mobile || user.email || '',
      email: user.mobile || user.email || '',
      avatar_url: user.avatar_url || '',
      reason: session.auth.verification_required
        ? '账号登录有效，但生成请求需要在登录窗口完成验证'
        : overview.window_limit_section?.usage_exhausted
          ? '豆包当前使用额度已耗尽'
          : '豆包登录有效',
      quota_windows: quota,
    }
  } catch (error) {
    return apiSessionHealthError(error)
  }
}

function doubaoQuotaWindows(overview) {
  return (overview.window_limit_section?.window_limit_groups || []).flatMap((group) =>
    (group.window_limits || []).flatMap((window) => {
      if (
        !Number.isFinite(window.used_percent) ||
        window.used_percent < 0 ||
        window.used_percent > 100
      )
        return []
      return [
        {
          name: window.window_type === 2 ? '近 7 天' : '当前时段',
          task_kind: 'text',
          remaining_percent: 100 - window.used_percent,
          reset_at: window.end_time > 0 ? new Date(window.end_time).toISOString() : null,
        },
      ]
    })
  )
}

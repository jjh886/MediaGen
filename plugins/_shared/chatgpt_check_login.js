// 只有认证资料完整且页面没有登录阻塞界面时，才允许宿主保存或恢复账号。
;(async () => {
  function result(loggedIn, status, reason, profile = {}) {
    return {
      loggedIn,
      status,
      reason,
      userId: profile.userId || '',
      username: profile.username || '',
      email: profile.email || '',
      avatarUrl: profile.avatarUrl || '',
      tier: profile.tier || '',
    }
  }

  function failed(status, reason) {
    return JSON.stringify(result(false, status, reason))
  }

  try {
    if (window.__mediagenChatGptHasBlockingLoginUi()) {
      return failed('login_expired', 'ChatGPT 页面仍在等待登录或身份验证')
    }
    const response = await fetch('/api/auth/session', {
      credentials: 'include',
      cache: 'no-store',
      headers: { accept: 'application/json' },
    })
    if (!response.ok) return failed('inconclusive', `登录检查 HTTP ${response.status}`)
    const session = await response.json()
    const user = session?.user || {}
    const account = session?.account || {}
    if (!session?.accessToken || !account?.id || !user?.email) {
      return failed('login_expired', 'ChatGPT 登录尚未真正完成')
    }
    if (window.__mediagenChatGptHasBlockingLoginUi()) {
      return failed('login_expired', 'ChatGPT 页面仍在等待登录或身份验证')
    }
    return JSON.stringify(
      result(true, 'available', 'ChatGPT 登录有效', {
        userId: user.id || account.id,
        username: user.name || user.email,
        email: user.email,
        avatarUrl: user.picture || user.image || '',
        tier: account.planType || 'free',
      })
    )
  } catch (_) {
    return failed('inconclusive', '暂时无法检查 ChatGPT 登录状态')
  }
})()

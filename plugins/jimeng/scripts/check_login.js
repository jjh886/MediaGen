// 即梦登录检测：优先用同源 passport/account/info/v2 校验 HttpOnly cookie。
;(async () => {
  const empty = {
    loggedIn: false,
    userId: '',
    username: '',
    email: '',
    avatarUrl: '',
    tier: '',
  }

  try {
    const resp = await fetch(
      '/passport/account/info/v2/?aid=513695&account_sdk_source=web&sdk_version=2.2.6',
      { credentials: 'include' }
    )
    if (!resp.ok) return JSON.stringify(empty)

    const data = await resp.json()
    const user = data?.data || {}
    const userId = String(user.user_id_str || user.user_id || '')
    const sessionExpired = /登录|请先登录|session expired|logout/i.test(
      document.body?.innerText || ''
    )
    if (!userId || sessionExpired) return JSON.stringify(empty)

    return JSON.stringify({
      loggedIn: true,
      userId,
      username: user.screen_name || user.name || '',
      email: user.email || user.mobile || '',
      avatarUrl: user.avatar_url || '',
      tier: 'free',
    })
  } catch (_) {
    return JSON.stringify(empty)
  }
})()

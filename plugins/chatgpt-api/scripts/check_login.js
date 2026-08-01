// ChatGPT API 插件仍通过一次性登录窗口授权；这里只确认真实用户会话，不保存秘密。
;(async () => {
  const empty = () =>
    JSON.stringify({
      loggedIn: false,
      userId: '',
      username: '',
      email: '',
      avatarUrl: '',
      tier: '',
    })
  try {
    const response = await fetch('/api/auth/session', {
      credentials: 'include',
      cache: 'no-store',
      headers: { accept: 'application/json' },
    })
    if (!response.ok) return empty()
    const session = await response.json()
    const user = session?.user || {}
    const account = session?.account || {}
    if (!session?.accessToken || !account?.id || !user?.email) return empty()
    return JSON.stringify({
      loggedIn: true,
      userId: user.id || account.id,
      username: user.name || user.email,
      email: user.email,
      avatarUrl: user.picture || user.image || '',
      tier: account.planType || 'free',
    })
  } catch (_) {
    return empty()
  }
})()

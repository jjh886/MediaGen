// ChatGPT 登录检测
// 优先从页面 bootstrap / auth session 提取用户信息。
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
  const fromSession = (session) => {
    const user = session?.user || {}
    const loggedIn = session?.authStatus === 'logged_in' || !!user.email
    if (!loggedIn || !user.email) return null
    return {
      loggedIn: true,
      userId: user.id || '',
      username: user.name || '',
      email: user.email || '',
      avatarUrl: user.picture || user.image || '',
      tier: session?.account?.planType || 'free',
    }
  }

  try {
    const bootstrapText = document.querySelector('#client-bootstrap')?.textContent || ''
    if (bootstrapText) {
      const bootstrap = JSON.parse(bootstrapText)
      const result = fromSession(
        bootstrap?.session ? bootstrap : { authStatus: bootstrap?.authStatus, ...bootstrap }
      )
      if (result) return JSON.stringify(result)
    }
  } catch (_) {}

  try {
    const resp = await fetch('/api/auth/session', {
      credentials: 'include',
      headers: { accept: 'application/json' },
    })
    if (resp.ok) {
      const session = await resp.json()
      const result = fromSession(session)
      if (result) return JSON.stringify(result)
    }
  } catch (_) {}

  try {
    const resp = await fetch('/backend-api/me', {
      credentials: 'include',
      headers: { accept: 'application/json' },
    })
    if (!resp.ok) return empty()
    const me = await resp.json()
    // ChatGPT 未登录/匿名会话也可能返回临时 id。必须拿到 email 才视为真正登录。
    const loggedIn = !!me?.email

    return JSON.stringify({
      loggedIn,
      userId: loggedIn ? me?.id || '' : '',
      username: loggedIn ? me?.name || '' : '',
      email: loggedIn ? me?.email || '' : '',
      avatarUrl: loggedIn ? me?.picture || '' : '',
      tier: loggedIn ? 'free' : '',
    })
  } catch (_) {
    return empty()
  }
})()

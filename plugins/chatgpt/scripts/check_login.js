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
  const asObject = (value) => {
    if (value && typeof value === 'object') return value
    return {}
  }
  const firstText = (values) => {
    for (const value of values) {
      if (value == null) continue
      const text = String(value).trim()
      if (text) return text
    }
    return ''
  }
  const hasLoggedInSession = (authStatus, email, userId) => {
    if (['logged_out', 'unauthenticated'].includes(authStatus)) return false
    if (!userId && !email) return false
    if (authStatus === 'logged_in') return true
    return Boolean(email)
  }
  const fromSession = (rawSession) => {
    // ChatGPT 新版页面把登录信息放在 client-bootstrap.session 下；
    // /api/auth/session 则会直接返回 session，本函数统一兼容这两种形状。
    const raw = asObject(rawSession)
    const session = asObject(raw.session ? raw.session : raw)
    const user = asObject(session.user ? session.user : raw.user)
    const account = asObject(session.account ? session.account : raw.account)
    const authStatus = firstText([raw.authStatus, session.authStatus])
    const userId = firstText([user.id, account.user_id, account.id])
    const email = firstText([user.email])
    if (!hasLoggedInSession(authStatus, email, userId)) return null
    return {
      loggedIn: true,
      userId,
      username: firstText([user.name]),
      email,
      avatarUrl: firstText([user.picture, user.image]),
      tier: firstText([account.planType]) || 'free',
    }
  }

  try {
    const bootstrapText = document.querySelector('#client-bootstrap')?.textContent || ''
    if (bootstrapText) {
      const bootstrap = JSON.parse(bootstrapText)
      const result = fromSession(bootstrap)
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

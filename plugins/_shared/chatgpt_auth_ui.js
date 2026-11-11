// ChatGPT 会把旧 session 数据与“重新选择账号”界面同时渲染。
// 两个 ChatGPT 插件统一使用这个平台规则，避免登录窗口过早关闭。
;(() => {
  const isVisible = (element) => {
    if (!element) return false
    const rect = element.getBoundingClientRect?.()
    const style = window.getComputedStyle?.(element)
    return Boolean(
      rect?.width > 0 &&
      rect?.height > 0 &&
      (!style || (style.display !== 'none' && style.visibility !== 'hidden'))
    )
  }

  window.__mediagenChatGptHasBlockingLoginUi = () => {
    if (location.hostname === 'auth.openai.com') return true
    const selectors = '[data-testid="login-button"], [data-testid="signup-button"]'
    if (Array.from(document.querySelectorAll(selectors)).some(isVisible)) return true
    return Array.from(document.querySelectorAll('[role="dialog"]')).some((dialog) => {
      const text = String(dialog.textContent || '')
      return isVisible(dialog) && /choose an account to continue|选择.*账号.*继续/i.test(text)
    })
  }

  window.__mediagenChatGptVerifiedSession = async () => {
    const response = await fetch('/api/auth/session', {
      credentials: 'include',
      cache: 'no-store',
      headers: { accept: 'application/json' },
    })
    if (!response.ok) throw new Error('AUTH_INCONCLUSIVE: 登录检查 HTTP ' + response.status)
    const session = await response.json()
    if (session?.error === 'RefreshAccessTokenError')
      throw new Error(
        'LOGIN_EXPIRED: 网页会话无法续期，请在远程浏览器退出 ChatGPT 后重新登录；删除 MediaGen 账号不会清除网页旧会话。'
      )
    if (session?.error || !session?.accessToken || !session?.account?.id || !session?.user?.email)
      throw new Error('LOGIN_EXPIRED: ChatGPT 登录尚未完成，未取得有效授权。')
    const checked = await fetch('/backend-api/me', {
      credentials: 'include',
      cache: 'no-store',
      headers: {
        accept: 'application/json',
        authorization: 'Bearer ' + session.accessToken,
        'chatgpt-account-id': session.account.id,
      },
    })
    if (checked.status === 401)
      throw new Error('LOGIN_EXPIRED: ChatGPT 实际接口拒绝了登录令牌，请退出网页账号后重新登录。')
    if (!checked.ok)
      throw new Error('AUTH_INCONCLUSIVE: ChatGPT 实际接口验证 HTTP ' + checked.status)
    const identity = await checked.json()
    if (!identity || typeof identity !== 'object' || (!identity.id && !identity.email))
      throw new Error('AUTH_INCONCLUSIVE: ChatGPT 实际接口未返回账号身份。')
    return session
  }
})()

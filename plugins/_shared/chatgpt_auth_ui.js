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
})()

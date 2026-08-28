;(() => {
  const loginButton = [...document.querySelectorAll('button')].find((button) =>
    /^(登录|扫码登录)$/.test((button.textContent || '').trim())
  )
  return {
    logged_in: !loginButton,
    status: loginButton ? 'login_expired' : 'available',
    reason: loginButton ? '请重新登录 Doubao Work' : 'Doubao Work 登录有效',
  }
})()

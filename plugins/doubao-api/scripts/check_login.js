;(() => {
  const avatar = document.querySelector('img[src*="user-avatar"], img[alt*="头像"]')
  const loginButton = [...document.querySelectorAll('button')].find((button) =>
    /^(登录|扫码登录)$/.test((button.textContent || '').trim())
  )
  return {
    logged_in: Boolean(avatar || !loginButton),
    status: avatar || !loginButton ? 'available' : 'login_expired',
    reason: avatar || !loginButton ? '豆包登录有效' : '请重新登录豆包',
  }
})()

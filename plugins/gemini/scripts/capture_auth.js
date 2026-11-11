;(async () => {
  const data = window.WIZ_global_data || {}
  if (!data.SNlM0e) throw new Error('Gemini 尚未完成登录')
  const label =
    document.querySelector('a[href*="SignOutOptions"]')?.getAttribute('aria-label') || ''
  return JSON.stringify({
    authuser: Number(location.pathname.match(/^\/u\/(\d+)\//)?.[1] || 0),
    email: label.match(/[\w.+-]+@[\w.-]+\.[A-Za-z]+/)?.[0] || '',
    fingerprint: await apiLoginFingerprint(),
  })
})()

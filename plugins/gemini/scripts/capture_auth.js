;(async () => {
  const account = await geminiBrowserAccount()
  if (account.logged_in !== true) throw new Error(account.reason || 'Gemini 尚未完成登录')
  return JSON.stringify({
    authuser: Number(location.pathname.match(/^\/u\/(\d+)\//)?.[1] || 0),
    ...account,
    fingerprint: await apiLoginFingerprint(),
  })
})()

// 豆包登录检测 + 用户信息提取
// 从 window._ROUTER_DATA 读取 accountInfo
// 返回标准 JSON: { loggedIn, userId, username, email, avatarUrl, tier }
;(() => {
  try {
    // 2026-04-22: 豆包将 accountInfo 嵌套进 chat_layout.chat_layout，兼容新旧两种结构
    const outer = window._ROUTER_DATA?.loaderData?.chat_layout || null
    const rd = outer?.chat_layout || outer
    const ai = rd?.accountInfo?.data || {}
    const userId = String(ai.user_id || '')
    const sessionExpired = /会话过期|重新登录/i.test(document.body.innerText)
    const loggedIn = userId.length > 0 && !sessionExpired
    return JSON.stringify({
      loggedIn,
      userId,
      username: ai.screen_name || ai.nick_name || '',
      email: ai.email || ai.mobile || '',
      avatarUrl: ai.avatar_url || '',
      tier: 'free',
    })
  } catch (e) {
    return JSON.stringify({
      loggedIn: false,
      userId: '',
      username: '',
      email: '',
      avatarUrl: '',
      tier: '',
    })
  }
})()

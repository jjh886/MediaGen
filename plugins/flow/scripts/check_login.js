// Flow (Google Labs) 登录检测 + 用户信息 + 积分提取
// 1. 通过 /fx/api/auth/session 检测登录态和用户名/邮箱
// 2. 通过 aisandbox-pa API 获取积分和会员等级
// 返回标准 JSON: { loggedIn, userId, username, email, avatarUrl, tier, credits }
;(async () => {
  try {
    const resp = await fetch('/fx/api/auth/session')
    const session = await resp.json()
    const loggedIn = !!session?.user?.email
    if (!loggedIn) {
      return JSON.stringify({
        loggedIn: false,
        userId: '',
        username: '',
        email: '',
        avatarUrl: '',
        tier: '',
        credits: 0,
      })
    }
    let credits = 0
    let tier = 'free'
    try {
      const cr = await fetch(
        'https://aisandbox-pa.googleapis.com/v1/credits?key=AIzaSyBtrm0o5ab1c-Ec8ZuLcGt3oJAA5VWt3pY',
        { credentials: 'include' }
      )
      const cd = await cr.json()
      credits = cd.credits || 0
      const sku = cd.sku || ''
      if (!sku.includes('FREEMIUM') && cd.userPaygateTier !== 'PAYGATE_TIER_NOT_PAID') {
        tier = 'pro'
      }
    } catch (e) {}
    return JSON.stringify({
      loggedIn: true,
      userId: '',
      username: session?.user?.name || '',
      email: session?.user?.email || '',
      avatarUrl: session?.user?.image || '',
      tier,
      credits,
    })
  } catch (e) {
    return JSON.stringify({
      loggedIn: false,
      userId: '',
      username: '',
      email: '',
      avatarUrl: '',
      tier: '',
      credits: 0,
    })
  }
})()

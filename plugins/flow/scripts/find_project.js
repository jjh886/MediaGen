// Flow: 在首页找到已有项目并导航，或新建项目
// 处理 3 种页面状态：
//   1. 营销页（未登录）→ 点击 "Create with Flow"
//   2. 已登录首页 → 等待项目列表加载 → 进入第一个项目
//   3. 已登录首页但无项目 → 点击 "新建项目"
// 返回: { project_url } 或 { action, project_url }
;(async () => {
  function sleep(ms) {
    return new Promise((r) => setTimeout(r, ms))
  }

  try {
    const url = window.location.href || ''
    const body = document.body?.innerText || ''

    // 情况 0: 被重定向到 Google 账号登录页 → Google session 已失效。
    // 早期拦下来，错误消息里带"登录"关键词，account_manager::classify_failure
    // 会匹配到 LoginState::LoginExpired 分支，让前端提示用户重新登录，
    // 而不是被兜底的"未匹配规则"分类成 60 秒冷却。
    if (
      url.includes('accounts.google.com') ||
      url.includes('/signin/') ||
      body.includes('使用 Google 账号登录') ||
      body.includes('已退出账号') ||
      body.includes('请选择账号')
    ) {
      return JSON.stringify({
        project_url: '',
        error: '登录已失效，请重新登录 Google 账号',
        login_expired: true,
        url,
      })
    }

    // 情况 1: 未登录营销页（包含 Pricing 等营销文案）
    if (body.includes('Create with Flow') && body.includes('Pricing')) {
      const btn = Array.from(document.querySelectorAll('button, a')).find((b) =>
        b.textContent?.includes('Create with Flow')
      )
      if (btn) {
        btn.click()
        return JSON.stringify({ action: 'clicked_create', project_url: '' })
      }
    }

    // 情况 2/3: 已登录，等待项目列表加载（最多 10 秒）
    for (let i = 0; i < 20; i++) {
      // 查找项目链接
      const projectLink = document.querySelector('a[href*="/project/"]')
      if (projectLink) {
        return JSON.stringify({ project_url: projectLink.href })
      }

      // 查找 "新建项目" 按钮（说明页面已加载完但没有项目）
      const newBtn = Array.from(document.querySelectorAll('button')).find((b) =>
        b.textContent?.includes('新建项目')
      )
      if (newBtn && i >= 4) {
        // 等至少 2 秒后再判断为无项目，避免项目列表还在加载
        newBtn.click()
        await sleep(3000)
        return JSON.stringify({ action: 'clicked_new_project', project_url: '' })
      }

      await sleep(500)
    }

    // 超时：再检查一次是否在项目页了（可能已经自动导航）
    const currentUrl = window.location.href
    if (currentUrl.includes('/project/')) {
      return JSON.stringify({ project_url: currentUrl })
    }

    // 调试：输出页面上有什么
    const allLinks = document.querySelectorAll('a')
    const linkTexts = Array.from(allLinks)
      .slice(0, 10)
      .map((a) => a.href?.substring(0, 80))
    const allBtns = document.querySelectorAll('button')
    const btnTexts = Array.from(allBtns)
      .slice(0, 10)
      .map((b) => b.textContent?.trim()?.substring(0, 40))
    const bodySnippet = document.body?.innerText?.substring(0, 200)
    return JSON.stringify({
      project_url: '',
      error: '未找到项目',
      debug: {
        url: window.location.href,
        title: document.title,
        links: linkTexts,
        buttons: btnTexts,
        body: bodySnippet,
      },
    })
  } catch (e) {
    return JSON.stringify({ project_url: '', error: String(e?.message || e) })
  }
})()

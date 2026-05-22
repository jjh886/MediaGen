// 豆包: 准备页面 — 关闭第一个可见弹窗 + 点击"新对话"
// 真人一般只关眼前那一个弹窗，不会扫描全页，所以只处理第一个。
// 返回: { prepared: true }
;(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const rnd = (a, b) => a + Math.random() * (b - a)

  // 1. 关闭第一个可见的广告/活动弹窗
  await sleep(rnd(500, 1200))
  const closeBtn = Array.from(
    document.querySelectorAll(
      '[class*="close"] svg, [aria-label="关闭"], [class*="dialog"] [class*="close"], [class*="modal"] [class*="close"]'
    )
  ).find((el) => el.offsetParent !== null)
  if (closeBtn) {
    const parent = closeBtn.closest('button, [role=button], div[class*=close]')
    if (parent) {
      parent.click()
    } else {
      closeBtn.click()
    }
    await sleep(rnd(400, 900))
  }

  // 2. 点击"新对话"按钮（如果在历史聊天页面）
  const allEls = document.querySelectorAll('button, [role="button"], a, div[class*="new"], span')
  for (const el of allEls) {
    const text = (el.textContent || '').trim()
    if (text === '新对话' || text === '+ 新对话') {
      el.click()
      await sleep(rnd(1000, 1800))
      break
    }
  }

  return JSON.stringify({ prepared: true })
})()

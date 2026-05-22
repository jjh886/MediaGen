// 豆包: 点击底部能力栏的"图像生成"按钮，切换到图像生成模式
// 实际 DOM: <button><svg/><div><div>图像生成</div></div></button>
// JS click 即可，不需要 CDP 鼠标事件（该按钮不需要 popover 触发）
// 返回: { switched: true } 或 { already: true }
;(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const rnd = (a, b) => a + Math.random() * (b - a)

  await sleep(rnd(500, 1200))

  // 检查是否已在图像生成模式（有 data-slate-editor 和"图像生成"标签）
  const slateEditor = document.querySelector('[data-slate-editor=true]')
  const imgGenTag = document.querySelector('[contenteditable=false]')
  if (slateEditor && imgGenTag && imgGenTag.textContent?.includes('图像生成')) {
    return JSON.stringify({ already: true })
  }

  // 在能力按钮栏找"图像生成"按钮
  const buttons = document.querySelectorAll('button')
  for (const btn of buttons) {
    const innerDivs = btn.querySelectorAll('div')
    for (const d of innerDivs) {
      if (d.textContent.trim() === '图像生成' && d.children.length === 0) {
        btn.click()
        await sleep(rnd(1500, 2500))
        return JSON.stringify({ switched: true })
      }
    }
  }

  return JSON.stringify({ error: '未找到图像生成按钮' })
})()

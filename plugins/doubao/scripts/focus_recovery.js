// 豆包: 关闭 popover 并恢复 Slate editor 焦点
// 图像生成模式的输入框是 div[data-slate-editor=true]（contenteditable），不是 textarea
// 比例选择后 popover 可能未关闭导致编辑器失焦
;(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const rnd = (a, b) => a + Math.random() * (b - a)

  // Escape 必须派发到 document：豆包图像生成输入区会把"焦点在输入区 + Escape"
  // 绑定为"退出图像生成模式"，派发到 activeElement 会沿焦点元素的处理链触发退出。
  // document 级别派发可以绕开输入区的 keydown 链，但依然被上层 Radix popover 监听捕获。
  const pressEscape = () => {
    document.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'Escape',
        code: 'Escape',
        bubbles: true,
        cancelable: true,
      })
    )
  }

  await sleep(rnd(200, 450))
  pressEscape()
  await sleep(rnd(200, 450))

  // 点击空白区域（body 点击用于关闭浮层）
  document.body.click()
  await sleep(rnd(400, 800))

  pressEscape()
  await sleep(rnd(200, 450))

  // 聚焦 Slate editor
  const editor = document.querySelector('[data-slate-editor=true]')
  if (editor) {
    editor.focus()
    await sleep(rnd(80, 180))

    // 将光标移到内容末尾
    const sel = window.getSelection()
    if (sel) {
      const range = document.createRange()
      range.selectNodeContents(editor)
      range.collapse(false)
      sel.removeAllRanges()
      sel.addRange(range)
    }
  }

  await sleep(rnd(200, 450))
  return JSON.stringify({ focused: !!editor })
})()

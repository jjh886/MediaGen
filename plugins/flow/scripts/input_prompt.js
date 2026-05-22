// Flow: 通过 Slate.js editor API 输入 prompt
// Slate.js 编辑器不响应 CDP 键盘事件和 execCommand，
// 必须通过 React fiber 找到 editor 实例，调用 editor.insertText()
// 输入: window.__frame64_params.prompt
;(async () => {
  const params = window.__frame64_params || {}
  const prompt = params.prompt || ''

  if (!prompt) {
    return JSON.stringify({ error: '提示词不能为空' })
  }

  try {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

    // 找到 Slate editor DOM 元素
    const editorEl = document.querySelector('div[role="textbox"][contenteditable="true"]')
    if (!editorEl) {
      return JSON.stringify({ error: '未找到编辑器' })
    }

    // 通过 React fiber 找到 Slate editor 实例
    const fiberKey = Object.keys(editorEl).find(
      (k) => k.startsWith('__reactFiber') || k.startsWith('__reactInternalInstance')
    )
    if (!fiberKey) {
      return JSON.stringify({ error: '未找到 React fiber' })
    }

    let editor = null
    let current = editorEl[fiberKey]
    for (let i = 0; i < 30 && current; i++) {
      const ms = current.memoizedState
      const mp = current.memoizedProps
      // Slate editor 有 children + apply + insertText 方法
      if (ms?.memoizedState?.current?.children && ms?.memoizedState?.current?.apply) {
        editor = ms.memoizedState.current
        break
      }
      if (mp?.editor?.children && mp?.editor?.apply) {
        editor = mp.editor
        break
      }
      current = current.return
    }

    if (!editor) {
      return JSON.stringify({ error: '未找到 Slate editor 实例' })
    }

    // 清空现有内容（如果有）
    const existingText = editor.children?.[0]?.children?.[0]?.text || ''
    if (existingText) {
      // 选中所有文本并删除
      const endOffset = existingText.length
      editor.selection = {
        anchor: { path: [0, 0], offset: 0 },
        focus: { path: [0, 0], offset: endOffset },
      }
      editor.deleteFragment()
    }

    // 设置光标到开头
    editor.selection = {
      anchor: { path: [0, 0], offset: 0 },
      focus: { path: [0, 0], offset: 0 },
    }

    // 插入新 prompt
    editor.insertText(prompt)

    await sleep(300)

    // 验证
    const finalText = editor.children?.[0]?.children?.[0]?.text || ''
    const domText = editorEl.textContent || ''

    return JSON.stringify({
      typed: true,
      slateText: finalText.substring(0, 60),
      domText: domText.substring(0, 60),
      match: finalText === domText,
    })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()

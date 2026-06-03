// Flow: 通过 Slate.js editor API 输入 prompt
// Slate.js 编辑器不响应 CDP 键盘事件和 execCommand，
// 必须通过 React fiber 找到 editor 实例，调用 editor.insertText()
// 输入: window.__mediagen_params.prompt
;(async () => {
  const params = window.__mediagen_params || {}
  const prompt = params.prompt || ''

  if (!prompt) {
    return JSON.stringify({ error: '提示词不能为空' })
  }

  try {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

    const findFiberKey = (el) =>
      Object.keys(el).find(
        (k) => k.startsWith('__reactFiber') || k.startsWith('__reactInternalInstance')
      )

    const resolveSlateEditor = (fiber) => {
      let current = fiber
      for (let i = 0; i < 30 && current; i++) {
        const stateEditor = current.memoizedState?.memoizedState?.current
        if (stateEditor?.children && stateEditor?.apply) return stateEditor

        const propsEditor = current.memoizedProps?.editor
        if (propsEditor?.children && propsEditor?.apply) return propsEditor
        current = current.return
      }
      return null
    }

    const replaceEditorText = (editor, text) => {
      const existingText = editor.children?.[0]?.children?.[0]?.text || ''
      editor.selection = {
        anchor: { path: [0, 0], offset: 0 },
        focus: { path: [0, 0], offset: existingText.length },
      }
      if (existingText) editor.deleteFragment()
      editor.selection = {
        anchor: { path: [0, 0], offset: 0 },
        focus: { path: [0, 0], offset: 0 },
      }
      editor.insertText(text)
    }

    // 找到 Slate editor DOM 元素，再通过 React fiber 取到 editor 实例。
    const editorEl = document.querySelector('div[role="textbox"][contenteditable="true"]')
    if (!editorEl) {
      return JSON.stringify({ error: '未找到编辑器' })
    }

    const fiberKey = findFiberKey(editorEl)
    if (!fiberKey) {
      return JSON.stringify({ error: '未找到 React fiber' })
    }

    const editor = resolveSlateEditor(editorEl[fiberKey])
    if (!editor) {
      return JSON.stringify({ error: '未找到 Slate editor 实例' })
    }

    replaceEditorText(editor, prompt)
    await sleep(300)

    const buildTypedResult = () => {
      const slateText = editor.children?.[0]?.children?.[0]?.text || ''
      const domText = editorEl.textContent || ''
      return {
        typed: true,
        slateText: slateText.substring(0, 60),
        domText: domText.substring(0, 60),
        match: slateText === domText,
      }
    }

    return JSON.stringify(buildTypedResult())
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()

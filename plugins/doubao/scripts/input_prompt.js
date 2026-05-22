// 豆包: 通过 Slate.js editor API 输入 prompt
//
// 无头模式下 CDP 键盘事件不被 Slate 的 beforeinput 监听器捕获，
// 所以依赖 React fiber 找到 editor 实例调 editor.insertText() 写入内容。
// 写完后派发一个不可取消的 input 事件补齐事件链（该事件不会触发 Slate 再次插入）。
//
// 注意：不要在 insertText 之前派发带数据的 paste / beforeinput 事件，
// Slate 的内置 handler 会把这些事件当成真实粘贴并再插一次，导致 prompt 被重复写入。
//
// 输入: window[Symbol.for('_s')].params.prompt
;(async () => {
  const SK = Symbol.for('_s')
  const s = window[SK] || {}
  const params = s.params || {}
  const prompt = params.prompt || ''

  if (!prompt) {
    return JSON.stringify({ error: '提示词不能为空' })
  }

  try {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
    const rnd = (a, b) => a + Math.random() * (b - a)

    const editorEl = document.querySelector('[data-slate-editor=true]')
    if (!editorEl) {
      // 现场快照：帮助判断 Rust 是在和哪个 tab/frame 对话
      const snapshot = {
        url: location.href,
        title: document.title,
        visibility: document.visibilityState,
        hidden: document.hidden,
        inIframe: window !== window.top,
        framesCount: window.frames.length,
        readyState: document.readyState,
        slateCount: document.querySelectorAll('[data-slate-editor=true]').length,
        textareaCount: document.querySelectorAll('textarea').length,
        textareaInfo: Array.from(document.querySelectorAll('textarea'))
          .map((t) => ({ placeholder: t.placeholder || '', visible: t.offsetParent !== null }))
          .slice(0, 3),
        contentEditableCount: document.querySelectorAll('[contenteditable=true]').length,
        hasImgGenLabel: Array.from(document.querySelectorAll('[contenteditable=false]')).some(
          (el) => (el.textContent || '').includes('图像生成')
        ),
        hasDialog: document.querySelectorAll(
          '[role=dialog], [class*="modal"]:not([class*="exited"])'
        ).length,
      }
      return JSON.stringify({ error: '未找到 Slate 编辑器', snapshot })
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

    // 1) 清空现有内容（防止多次调用或残留占位文本）
    const existingText = editor.children?.[0]?.children?.[0]?.text || ''
    if (existingText) {
      editor.selection = {
        anchor: { path: [0, 0], offset: 0 },
        focus: { path: [0, 0], offset: existingText.length },
      }
      editor.deleteFragment()
    }

    editor.selection = {
      anchor: { path: [0, 0], offset: 0 },
      focus: { path: [0, 0], offset: 0 },
    }

    await sleep(rnd(30, 100))

    // 2) 写入 prompt（Slate 会自己触发 onChange / 内部 input 流）
    editor.insertText(prompt)

    // 3) 派发不可取消的 input 事件补齐事件链（cancelable:false 不会触发 Slate 再次插入）
    editorEl.dispatchEvent(
      new InputEvent('input', {
        bubbles: true,
        cancelable: false,
        inputType: 'insertText',
        data: prompt,
      })
    )

    await sleep(rnd(150, 400))

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

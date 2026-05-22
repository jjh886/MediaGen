// 即梦使用 Tiptap/ProseMirror；输入必须触发编辑器状态更新，不能只改 DOM 文本。
;(async () => {
  const SK = Symbol.for('_jimeng')
  const state = window[SK] || {}
  const prompt = String(state.params?.prompt || '').trim()
  const ratio = String(state.params?.ratio || '9:16').trim() || '9:16'
  const finalPrompt = prompt.includes(ratio) ? prompt : `${prompt}\n画面比例：${ratio}`
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

  const isVisible = (el) => {
    if (!el) return false
    const rect = el.getBoundingClientRect?.()
    if (!rect || rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle?.(el)
    return (
      !style || (style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0')
    )
  }

  const isInsideResultStream = (el) => {
    const selector = [
      '[class*="work-detail"]',
      '[class*="virtual-list"]',
      '[class*="scroll-list"]',
      '[class*="ai-generated-record"]',
      '[class*="record-box-wrapper"]',
      '[class*="responsive-image-grid"]',
      '[class*="responsive-video-grid"]',
    ].join(', ')
    return !!el?.closest?.(selector)
  }

  const isUsableEditor = (el) => {
    if (!isVisible(el) || isInsideResultStream(el)) return -1
    if (el.closest?.('[role="dialog"], [aria-modal="true"], [class*="modal"], [class*="preview"]'))
      return -1
    return 0
  }

  const editorContextText = (el) =>
    String(el.className || '') +
    ' ' +
    String(el.parentElement?.className || '') +
    ' ' +
    String(
      el.closest?.('[class*="main-content"], [class*="prompt-editor"], [class*="layout-"]')
        ?.className || ''
    )

  const scoreEditor = (el) => {
    if (isUsableEditor(el) < 0) return -1
    const rect = el.getBoundingClientRect?.() || {}
    let score = 0
    const contextText = editorContextText(el)
    if (/ProseMirror/.test(contextText)) score += 50
    if (/prompt-editor|main-content|layout-/.test(contextText)) score += 40
    if (rect.bottom > window.innerHeight * 0.35) score += 15
    if (rect.width > 240) score += 10
    return score
  }

  const findEditor = () =>
    [
      ...document.querySelectorAll(
        '.ProseMirror[contenteditable="true"], div[role="textbox"][contenteditable="true"]'
      ),
    ]
      .map((el) => ({ el, score: scoreEditor(el) }))
      .filter((item) => item.score >= 0)
      .sort((a, b) => b.score - a.score)[0]?.el || null

  const htmlFromText = (text) =>
    text
      .split(/\n+/)
      .map(
        (line) =>
          `<p>${line.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c])}</p>`
      )
      .join('')

  const inputEvent = (type, options = {}) => {
    try {
      return new InputEvent(type, { bubbles: true, cancelable: true, ...options })
    } catch (_) {
      return new Event(type, { bubbles: true, cancelable: true })
    }
  }

  const dispatchInputEvents = (el, data) => {
    el.dispatchEvent(inputEvent('beforeinput', { inputType: 'insertText', data }))
    el.dispatchEvent(inputEvent('input', { inputType: 'insertText', data }))
    el.dispatchEvent(new Event('change', { bubbles: true }))
    el.dispatchEvent(new KeyboardEvent('keyup', { bubbles: true, key: 'Process' }))
  }

  const selectContents = (el) => {
    try {
      const selection = window.getSelection?.()
      const range = document.createRange()
      range.selectNodeContents(el)
      selection?.removeAllRanges()
      selection?.addRange(range)
    } catch (_) {}
  }

  const readText = (el) => (el?.textContent || '').replace(/\s+/g, ' ').trim()
  const isFilled = (el) => {
    const text = readText(el)
    const needle = prompt.slice(0, Math.min(prompt.length, 20))
    return !!needle && text.includes(needle)
  }

  const fillOnce = async (editor) => {
    editor.focus()
    editor.click?.()
    selectContents(editor)
    try {
      document.execCommand?.('delete', false)
    } catch (_) {}
    editor.textContent = ''
    dispatchInputEvents(editor, '')

    let inserted = false
    try {
      inserted = !!document.execCommand?.('insertText', false, finalPrompt)
    } catch (_) {
      inserted = false
    }
    if (!inserted || !isFilled(editor)) {
      editor.innerHTML = htmlFromText(finalPrompt)
    }
    dispatchInputEvents(editor, finalPrompt)
    await sleep(350)
  }

  try {
    if (!prompt) return JSON.stringify({ error: '提示词不能为空' })

    let editor = null
    for (let attempt = 1; attempt <= 4; attempt += 1) {
      editor = findEditor()
      if (!editor) {
        await sleep(300)
        continue
      }
      await fillOnce(editor)
      if (isFilled(editor)) {
        state.last_prompt_text = finalPrompt
        state.prompt_typed_at = Date.now()

        // ProseMirror 偶尔会在下一轮 render 里回滚，短时间内做一次保活确认。
        for (let i = 0; i < 4; i += 1) {
          await sleep(180)
          const latest = findEditor() || editor
          if (!isFilled(latest)) await fillOnce(latest)
        }

        const domText = readText(findEditor() || editor)
        return JSON.stringify({
          typed: true,
          length: finalPrompt.length,
          preview: domText.slice(0, 80),
        })
      }
    }

    return JSON.stringify({ error: '提示词写入后未在编辑器中生效', hasEditor: !!editor })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()

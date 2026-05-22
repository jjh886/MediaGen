// 等待即梦提交按钮可点击，并打上稳定 selector 供 browser.click 使用。
;(async () => {
  const SK = Symbol.for('_jimeng')
  const state = window[SK] || {}
  const markerAttr = 'data-frame64-jimeng-submit'
  const expectedRefs = Number(state.params?.refCount || 0)
  const expectedRatio = String(state.params?.ratio || '').trim()
  const expectedModel = String(state.params?.modelDisplayName || state.params?.modelId || '').trim()
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

  const isInsideResultStream = (el) =>
    !!el?.closest?.(
      [
        '[class*="work-detail"]',
        '[class*="virtual-list"]',
        '[class*="scroll-list"]',
        '[class*="ai-generated-record"]',
        '[class*="record-box-wrapper"]',
        '[class*="responsive-image-grid"]',
        '[class*="responsive-video-grid"]',
      ].join(', ')
    )

  const classOf = (el) => String(el?.className || '')
  const textOf = (el) =>
    [el?.textContent, el?.getAttribute?.('aria-label'), el?.getAttribute?.('title')]
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim()
  const normalizeText = (value) =>
    String(value || '')
      .replace(/\s+/g, ' ')
      .trim()
  const compactText = (value) => normalizeText(value).replace(/\s+/g, '')
  const expectedFinalPrompt = () => {
    const typed = String(state.last_prompt_text || '').trim()
    if (typed) return typed
    const prompt = String(state.params?.prompt || '').trim()
    if (!prompt) return ''
    const ratio = expectedRatio || '9:16'
    return prompt.includes(ratio) ? prompt : `${prompt}\n画面比例：${ratio}`
  }
  const hasExpectedPrompt = (editor) => {
    const expected = normalizeText(expectedFinalPrompt())
    if (!editor || !expected) return false
    const actual = normalizeText(editor.textContent || '')
    const needle = expected.slice(0, Math.min(expected.length, 28))
    return (
      !!needle && (actual.includes(needle) || compactText(actual).includes(compactText(needle)))
    )
  }

  const escapeHtml = (value) =>
    String(value || '').replace(
      /[&<>]/g,
      (ch) =>
        ({
          '&': '&amp;',
          '<': '&lt;',
          '>': '&gt;',
        })[ch] || ch
    )

  const promptToHtml = (value) =>
    String(value || '')
      .split(/\n+/)
      .map((line) => `<p>${escapeHtml(line)}</p>`)
      .join('')

  const makeInputEvent = (type, data) => {
    try {
      return new InputEvent(type, {
        bubbles: true,
        cancelable: true,
        inputType: 'insertText',
        data,
      })
    } catch (_) {
      return new Event(type, { bubbles: true, cancelable: true })
    }
  }

  const dispatchEditorEvents = (editor, data) => {
    editor.dispatchEvent(makeInputEvent('beforeinput', data))
    editor.dispatchEvent(makeInputEvent('input', data))
    editor.dispatchEvent(new Event('change', { bubbles: true }))
    editor.dispatchEvent(new KeyboardEvent('keyup', { bubbles: true, key: 'Process' }))
  }

  const selectEditorContents = (editor) => {
    try {
      const range = document.createRange()
      range.selectNodeContents(editor)
      const selection = window.getSelection?.()
      selection?.removeAllRanges()
      selection?.addRange(range)
    } catch (_) {}
  }

  const ensurePrompt = async (editor) => {
    const finalPrompt = expectedFinalPrompt()
    if (!editor || !finalPrompt) return false
    if (hasExpectedPrompt(editor)) return true

    editor.focus?.()
    editor.click?.()
    selectEditorContents(editor)
    try {
      document.execCommand?.('delete', false)
    } catch (_) {}
    editor.textContent = ''
    dispatchEditorEvents(editor, '')

    let inserted = false
    try {
      inserted = !!document.execCommand?.('insertText', false, finalPrompt)
    } catch (_) {
      inserted = false
    }
    if (!inserted || !hasExpectedPrompt(editor)) {
      editor.innerHTML = promptToHtml(finalPrompt)
    }
    dispatchEditorEvents(editor, finalPrompt)
    state.last_prompt_text = finalPrompt
    state.prompt_repaired_at = Date.now()
    await sleep(250)
    return hasExpectedPrompt(editor)
  }

  const isDisabled = (button) => {
    const cls = classOf(button)
    return (
      !!button?.disabled ||
      button?.getAttribute?.('aria-disabled') === 'true' ||
      /disabled|lv-btn-disabled/.test(cls)
    )
  }

  const scoreEditor = (el) => {
    if (!isVisible(el) || isInsideResultStream(el)) return -1
    if (el.closest?.('[role="dialog"], [aria-modal="true"], [class*="modal"], [class*="preview"]'))
      return -1
    const rect = el.getBoundingClientRect?.() || {}
    const contextClass = [
      classOf(el),
      classOf(el.parentElement),
      classOf(
        el.closest?.('[class*="main-content"], [class*="prompt-editor"], [class*="layout-"]')
      ),
    ].join(' ')
    let score = 0
    if (/ProseMirror/.test(contextClass)) score += 50
    if (/prompt-editor|main-content|layout-/.test(contextClass)) score += 60
    if (rect.width > 240) score += 10
    if (rect.bottom > window.innerHeight * 0.3) score += 10
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

  const isUsableComposerRoot = (root, editor) =>
    !!root && root.contains(editor) && isVisible(root) && !isInsideResultStream(root)

  const composerRootFeatureScore = (root) => {
    let score = 0
    if (root.querySelector?.('input[type="file"][accept*="image"]')) score += 30
    if (
      root.querySelector?.(
        '[class*="toolbar-actions"], [class*="collapsed-submit-button-container"]'
      )
    )
      score += 70
    if (root.querySelector?.('button[class*="submit-button"], button.lv-btn-primary')) score += 70
    return score
  }

  const scoreComposerRoot = (root, editor) => {
    if (!isUsableComposerRoot(root, editor)) return -1
    const rect = root.getBoundingClientRect?.() || {}
    let score = composerRootFeatureScore(root)
    if (/layout-|content-generator|dimension-layout/.test(classOf(root))) score += 20
    if (rect.width > 300 && rect.height > 80) score += 10
    return score
  }

  const findComposerRoot = (editor) => {
    if (!editor) return null
    const candidates = []
    let current = editor
    let depth = 0
    while (current && current !== document.body && depth < 10) {
      candidates.push({ node: current, score: scoreComposerRoot(current, editor) })
      current = current.parentElement
      depth += 1
    }
    candidates.push({ node: document.body, score: 0 })
    candidates.sort((a, b) => b.score - a.score)
    return candidates[0]?.node || document.body
  }

  const summarizeCandidates = (items) =>
    items.slice(0, 10).map(({ button, score, disabled }) => {
      const rect = button.getBoundingClientRect?.() || {}
      return {
        score,
        disabled,
        className: classOf(button).slice(0, 140),
        parentClass: classOf(button.parentElement).slice(0, 100),
        text: textOf(button).slice(0, 50),
        rect: {
          x: Math.round(rect.x || rect.left || 0),
          y: Math.round(rect.y || rect.top || 0),
          w: Math.round(rect.width || 0),
          h: Math.round(rect.height || 0),
        },
      }
    })

  const isUsableSubmitButton = (button) => {
    if (!isVisible(button)) return null
    if (isInsideResultStream(button)) return null
    if (
      button.closest?.('[role="dialog"], [aria-modal="true"], [class*="modal"], [class*="preview"]')
    )
      return null
    const text = textOf(button)
    if (/上传|添加|参考|删除|做同款|升级会员|会员得积分/.test(text)) return null
    return true
  }

  const submitButtonBaseScore = (button, root) => {
    const cls = classOf(button)
    let score = 0
    if (root?.contains?.(button)) score += 90
    if (/submit-button/.test(cls)) score += 120
    if (/lv-btn-primary/.test(cls)) score += 60
    if (/shape-circle|icon-only/.test(cls)) score += 30
    if (button.closest?.('[class*="toolbar-actions"]')) score += 120
    if (button.closest?.('[class*="collapsed-submit-button-container"]')) score += 90
    return score
  }

  const submitButtonPositionPenalty = (button, editor) => {
    const er = editor?.getBoundingClientRect?.()
    const br = button.getBoundingClientRect?.()
    if (!er || !br) return 0
    let score = 0
    if (br.left >= er.left - 60) score += 20
    if (br.top >= er.top - 120) score += 10
    if (br.left > er.left + er.width * 0.55) score += 20
    const distance =
      Math.abs(br.left + br.width / 2 - (er.left + er.width)) +
      Math.abs(br.top + br.height / 2 - (er.top + er.height))
    return score - Math.min(Math.round(distance / 250), 20)
  }

  const scoreSubmitButton = (button, root, editor) => {
    if (!isUsableSubmitButton(button)) return null
    return submitButtonBaseScore(button, root) + submitButtonPositionPenalty(button, editor)
  }

  const findSubmitButton = (editor = findEditor()) => {
    if (!editor) return { button: null, candidates: [], root: null }
    const root = findComposerRoot(editor)
    const selectors = [
      '[class*="toolbar-actions"] button',
      '[class*="collapsed-submit-button-container"] button',
      'button[class*="submit-button"]',
      'button.lv-btn-primary',
      'button[type="button"]',
    ].join(', ')
    const nodes = [
      ...new Set([
        ...root.querySelectorAll(selectors),
        ...document.querySelectorAll(
          '[class*="toolbar-actions"] button, [class*="collapsed-submit-button-container"] button'
        ),
      ]),
    ]
    const scored = nodes
      .map((button) => ({
        button,
        score: scoreSubmitButton(button, root, editor) || 0,
        disabled: isDisabled(button),
      }))
      .filter((item) => item.score > 0)
      .sort((a, b) => b.score - a.score)
    const best = scored.find((item) => item.score >= 140 && !item.disabled)?.button || null
    return { button: best, candidates: summarizeCandidates(scored), root }
  }

  try {
    document
      .querySelectorAll(`[${markerAttr}="true"]`)
      .forEach((el) => el.removeAttribute(markerAttr))

    const deadline = Date.now() + 60000
    let last = {
      button: false,
      prompt: false,
      ratio: false,
      model: false,
      interceptor: false,
      uploaded: 0,
      candidates: [],
    }
    while (Date.now() < deadline) {
      if (/\/ai-tool\/work-detail\//.test(location.pathname)) {
        return JSON.stringify({ error: `即梦页面已离开生成页: ${location.href}` })
      }
      let editor = findEditor()
      let promptReady = hasExpectedPrompt(editor)
      if (!promptReady && editor) {
        promptReady = await ensurePrompt(editor)
        editor = findEditor() || editor
      }
      const editorText = normalizeText(editor?.textContent || '')
      const ratioReady =
        !!expectedRatio && editorText.includes(expectedRatio) && state.ratio_ready === true
      const modelReady = !expectedModel || state.model_ready === true
      const interceptorReady = state.fetchHooked === true
      const uploaded = Array.isArray(state.uploaded_refs) ? state.uploaded_refs.length : 0
      const domCards = Number(state.uploaded_dom_cards || 0)
      const refsReady =
        expectedRefs <= 0 ||
        uploaded >= expectedRefs ||
        state.uploads_ready === true ||
        domCards >= expectedRefs
      const submit = findSubmitButton(editor)
      const button = submit.button
      last = {
        button: !!button,
        prompt: promptReady,
        ratio: ratioReady,
        model: modelReady,
        interceptor: interceptorReady,
        uploaded,
        domCards,
        repaired: !!state.prompt_repaired_at,
        candidates: submit.candidates,
      }
      state.submit_diagnostics = last

      if (button && promptReady && ratioReady && modelReady && interceptorReady && refsReady) {
        button.setAttribute(markerAttr, 'true')
        return JSON.stringify({
          ready: true,
          selector: `[${markerAttr}="true"]`,
          expectedRefs,
          uploaded,
          prompt: true,
          ratio: expectedRatio,
          model: expectedModel,
        })
      }
      await sleep(500)
    }
    return JSON.stringify({
      error: `即梦提交按钮未就绪 button=${last.button}, prompt=${last.prompt}, ratio=${last.ratio}, model=${last.model}, interceptor=${last.interceptor}, uploaded=${last.uploaded}/${expectedRefs}, domCards=${last.domCards || 0}`,
      candidates: last.candidates,
    })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()

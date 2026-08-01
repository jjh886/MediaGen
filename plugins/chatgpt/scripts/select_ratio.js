// ChatGPT: locate and mark the aspect-ratio trigger button.
// The actual clicks are done by browser.click / browser.click_text so they use CDP
// trusted mouse events, matching the current project convention used by browser plugins.
;(async () => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || (window[SK] = {})
  const dom = state.dom
  const params = state.params || {}

  const normalizeRatio = (value) => {
    const v = String(value || '').trim()
    return ['auto', '9:16', '16:9', '1:1', '3:4', '4:3', '2:1'].includes(v) ? v : '9:16'
  }

  const targetRatio = normalizeRatio(params.ratio)
  state.params = { ...params, ratio: targetRatio }

  const ratioAliases = {
    '9:16': ['9:16', '9 / 16', '9x16', '9×16', 'portrait', 'vertical', 'story', '故事'],
    '16:9': ['16:9', '16 / 9', '16x9', '16×9', 'landscape', 'horizontal', 'widescreen', '宽屏'],
    '1:1': ['1:1', '1 / 1', '1x1', '1×1', 'square', '方形'],
    '3:4': ['3:4', '3 / 4', '3x4', '3×4', 'portrait 3:4', '竖版'],
    '4:3': ['4:3', '4 / 3', '4x3', '4×3', 'landscape 4:3', '横屏'],
  }
  const anyRatioTokens = [...new Set(Object.values(ratioAliases).flat())]
  const ratioKeywordTokens = ['比例', '宽高比', 'aspect ratio', 'aspect', 'size', '尺寸']
  const nonRatioTokens = ['model-switcher', 'model switcher', '模型选择器', 'chatgpt', 'gpt-']
  const triggerTextTokens = [
    'auto',
    '方形',
    '竖版',
    '故事',
    '横屏',
    '宽屏',
    '1:1',
    '3:4',
    '9:16',
    '4:3',
    '16:9',
  ]
  const markerAttr = 'data-mediagen-cgpt-ratio-trigger'

  const matchesRatio = (el, ratio) => {
    const haystack = dom?.textOf(el) || ''
    if (!haystack) return false
    return (ratioAliases[ratio] || []).some((token) =>
      haystack.includes(String(token).toLowerCase())
    )
  }

  const findComposerRoot = () => {
    return dom?.findComposerRoot({ requireImageish: true, imageTokens: triggerTextTokens })
  }

  const isHistoryCandidate = (el) => {
    const testid = String(el.getAttribute('data-testid') || '').toLowerCase()
    return testid.includes('history-item') || testid.includes('conversation-item')
  }

  const ratioEvidenceScore = (el) => {
    const text = dom?.textOf(el) || ''
    if (!text) return 0
    if (nonRatioTokens.some((token) => text.includes(token.toLowerCase()))) return 0

    let score = 0
    if (triggerTextTokens.some((token) => text === token.toLowerCase())) score += 10
    if (triggerTextTokens.some((token) => text.startsWith(`${String(token).toLowerCase()} `)))
      score += 8
    if (ratioKeywordTokens.some((token) => text.includes(token.toLowerCase()))) score += 4
    if (anyRatioTokens.some((token) => text.includes(String(token).toLowerCase()))) score += 2
    if (matchesRatio(el, targetRatio)) score += 3
    return score
  }

  const scoreRatioCandidate = (el) => {
    const score = ratioEvidenceScore(el)
    // `aria-haspopup` 只能作为加分项；模型选择器也有菜单，不能单独作为比例证据。
    return score > 0 && el.getAttribute('aria-haspopup') ? score + 3 : score
  }

  const collectRatioCandidates = (root) =>
    dom.collectInteractive(root).filter((el) => !isHistoryCandidate(el))

  const findRatioTrigger = () => {
    const candidates = collectRatioCandidates(findComposerRoot())
    const scored = candidates
      .map((el) => {
        const score = scoreRatioCandidate(el)
        return score > 0 ? { el, score } : null
      })
      .filter(Boolean)
      .sort((a, b) => b.score - a.score)

    return scored[0]?.el || null
  }

  try {
    if (!dom) {
      return JSON.stringify({
        selected: false,
        ratio: targetRatio,
        fallback: 'prompt_only',
        reason: 'dom_helper_missing',
        trigger_selector: '',
      })
    }

    dom.clearMarker(markerAttr)

    const trigger = findRatioTrigger()
    if (!trigger) {
      return JSON.stringify({
        selected: false,
        ratio: targetRatio,
        fallback: 'prompt_only',
        reason: 'ratio_control_not_found',
        trigger_selector: '',
      })
    }

    trigger.setAttribute(markerAttr, 'true')

    return JSON.stringify({
      prepared: true,
      ratio: targetRatio,
      strategy: 'trigger_marked',
      trigger_text: dom.textOf(trigger),
      trigger_selector: `[${markerAttr}="true"]`,
    })
  } catch (e) {
    return JSON.stringify({
      prepared: false,
      ratio: targetRatio,
      fallback: 'prompt_only',
      error: String(e?.message || e),
      trigger_selector: '',
    })
  }
})()

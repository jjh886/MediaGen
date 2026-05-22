// 即梦比例选择准备。只定位并标记比例触发器，真实点击由 browser.click 执行。
;(async () => {
  const SK = Symbol.for('_jimeng')
  const state = window[SK] || {}
  const targetRatio = String(state.params?.ratio || '9:16').trim() || '9:16'
  const markerAttr = 'data-frame64-jimeng-ratio-trigger'

  const isVisible = (el) => {
    if (!el) return false
    const rect = el.getBoundingClientRect?.()
    if (!rect || rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle?.(el)
    return (
      !style || (style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0')
    )
  }

  const textOf = (el) =>
    [el?.textContent, el?.getAttribute?.('aria-label'), el?.getAttribute?.('title')]
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim()

  const findRatioButton = () => {
    const buttons = [...document.querySelectorAll('button, [role="button"]')].filter(isVisible)
    const candidates = buttons
      .map((button) => {
        const text = textOf(button)
        if (/提交|生成|上传|删除/.test(text)) return null
        let score = 0
        if (text.includes(targetRatio)) score += 100
        if (/\d+\s*:\s*\d+/.test(text)) score += 80
        if (/智能比例|选择比例|高清\s*2K|超清\s*4K/.test(text)) score += 60
        if (/toolbar-button|lv-btn-secondary/.test(button.className || '')) score += 10
        return score > 0 ? { button, text, score } : null
      })
      .filter(Boolean)
      .sort((a, b) => b.score - a.score)
    return candidates[0]?.button || null
  }

  try {
    document
      .querySelectorAll(`[${markerAttr}="true"]`)
      .forEach((el) => el.removeAttribute(markerAttr))

    const button = findRatioButton()
    if (!button) {
      state.ratio_ready = false
      return JSON.stringify({
        prepared: false,
        ratio: targetRatio,
        reason: '未找到比例按钮',
        trigger_selector: '',
      })
    }
    const currentText = textOf(button)
    if (currentText.includes(targetRatio)) {
      state.ratio_ready = true
      state.ratio_selected_text = currentText
      return JSON.stringify({
        prepared: true,
        ratio: targetRatio,
        already: true,
        currentText,
        trigger_selector: '',
      })
    }
    state.ratio_ready = false
    button.setAttribute(markerAttr, 'true')
    return JSON.stringify({
      prepared: true,
      ratio: targetRatio,
      target_text: targetRatio,
      currentText,
      trigger_selector: `[${markerAttr}="true"]`,
    })
  } catch (e) {
    state.ratio_ready = false
    return JSON.stringify({
      prepared: false,
      ratio: targetRatio,
      reason: String(e?.message || e),
      trigger_selector: '',
    })
  }
})()

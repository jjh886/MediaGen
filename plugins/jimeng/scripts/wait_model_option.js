// 等待即梦模型下拉展开，定位目标模型选项并写入稳定 selector。
;(async () => {
  const SK = Symbol.for('_jimeng')
  const state = window[SK] || {}
  const rawTarget = String(state.params?.modelDisplayName || state.params?.modelId || '').trim()
  const markerAttr = 'data-mediagen-jimeng-model-option'
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

  const isVisible = (el) => {
    const rect = el?.getBoundingClientRect?.()
    if (!rect || rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle?.(el)
    if (!style) return true
    return (
      style.display !== 'none' &&
      style.visibility !== 'hidden' &&
      style.opacity !== '0' &&
      style.pointerEvents !== 'none'
    )
  }

  const textOf = (el) => {
    const imgAlts = el?.querySelectorAll
      ? [...el.querySelectorAll('img[alt]')].map((img) => img.getAttribute('alt') || '')
      : []
    return [
      el?.textContent,
      el?.getAttribute?.('aria-label'),
      el?.getAttribute?.('title'),
      el?.getAttribute?.('alt'),
      ...imgAlts,
    ]
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim()
  }

  const normalizeModelName = (value) => {
    let text = String(value || '').trim()
    text = text.replace(/^jimeng[_\s-]*/i, '')
    text = text.replace(/_/g, ' ')
    text = text.replace(/图片\s*([0-9])/g, '图片$1')
    return text.replace(/\s+/g, '').toLowerCase()
  }

  const displayTarget = (value) => {
    let text = String(value || '')
      .trim()
      .replace(/^jimeng[_\s-]*/i, '')
      .replace(/_/g, ' ')
    text = text.replace(/图片\s*([0-9])/g, '图片 $1')
    return text.replace(/\s+/g, ' ').trim()
  }

  const target = displayTarget(rawTarget)
  const targetKey = normalizeModelName(rawTarget)
  if (state.model_ready === true) {
    return JSON.stringify({
      found: false,
      skipped: true,
      target,
      reason: '模型已是目标项',
      selector: '',
    })
  }
  const findOption = () => {
    const options = [
      ...document.querySelectorAll(
        '.lv-select-popup [role="option"], [role="listbox"] [role="option"], .lv-select-option, li[role="option"]'
      ),
    ].filter(isVisible)
    return options.find((node) => {
      const text = textOf(node)
      if (/图片生成/.test(text)) return false
      const key = normalizeModelName(text)
      return key === targetKey || key.includes(targetKey)
    })
  }

  try {
    document
      .querySelectorAll(`[${markerAttr}="true"]`)
      .forEach((el) => el.removeAttribute(markerAttr))
    if (!targetKey)
      return JSON.stringify({ found: false, target, reason: '未指定即梦模型', selector: '' })

    for (let i = 0; i < 24; i++) {
      const option = findOption()
      if (option) {
        option.setAttribute(markerAttr, 'true')
        return JSON.stringify({
          found: true,
          target,
          target_text: target,
          option_text: textOf(option),
          selector: `[${markerAttr}="true"]`,
        })
      }
      await sleep(250)
    }
    return JSON.stringify({ found: false, target, reason: '未找到目标模型选项', selector: '' })
  } catch (e) {
    return JSON.stringify({ found: false, target, reason: String(e?.message || e), selector: '' })
  }
})()

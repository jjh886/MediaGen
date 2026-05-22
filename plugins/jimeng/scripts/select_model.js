// 即梦模型选择准备。只定位并标记模型下拉触发器，真实点击由 browser.click 执行。
;(async () => {
  const SK = Symbol.for('_jimeng')
  const state = window[SK] || {}
  const rawTarget = String(state.params?.modelDisplayName || state.params?.modelId || '').trim()
  const markerAttr = 'data-frame64-jimeng-model-trigger'

  const isVisible = (el) => {
    if (!el) return false
    const rect = el.getBoundingClientRect?.()
    if (!rect || rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle?.(el)
    return (
      !style || (style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0')
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
    text = text.replace(/\s+/g, '')
    return text.toLowerCase()
  }

  const displayTarget = (value) => {
    let text = String(value || '')
      .trim()
      .replace(/^jimeng[_\s-]*/i, '')
      .replace(/_/g, ' ')
    text = text.replace(/图片\s*([0-9])/g, '图片 $1')
    return text.replace(/\s+/g, ' ').trim()
  }

  const findModelSelect = () => {
    const nodes = [
      ...document.querySelectorAll('[role="combobox"], .lv-select, button, [role="button"]'),
    ].filter(isVisible)
    return nodes.find((node) => {
      const text = textOf(node)
      return (
        /图片\s*\d|Lite|Seedream/i.test(text) &&
        !/图片生成/.test(text) &&
        !/\d+\s*:\s*\d+/.test(text)
      )
    })
  }

  if (!rawTarget) {
    state.model_ready = true
    return JSON.stringify({ selected: false, skipped: true, reason: '未指定即梦模型' })
  }

  const target = displayTarget(rawTarget)
  const targetKey = normalizeModelName(rawTarget)
  try {
    document
      .querySelectorAll(`[${markerAttr}="true"]`)
      .forEach((el) => el.removeAttribute(markerAttr))

    const currentSelect = findModelSelect()
    const currentText = textOf(currentSelect)
    if (normalizeModelName(currentText).includes(targetKey)) {
      state.model_ready = true
      state.model_selected_text = currentText
      return JSON.stringify({
        prepared: true,
        already: true,
        target,
        currentText,
        trigger_selector: '',
      })
    }
    if (!currentSelect) {
      state.model_ready = false
      return JSON.stringify({
        prepared: false,
        target,
        reason: '未找到即梦模型选择器，生成请求会由拦截器兜底修正',
        trigger_selector: '',
      })
    }

    state.model_ready = false
    currentSelect.setAttribute(markerAttr, 'true')
    return JSON.stringify({
      prepared: true,
      target,
      target_key: targetKey,
      currentText,
      trigger_selector: `[${markerAttr}="true"]`,
    })
  } catch (e) {
    return JSON.stringify({
      prepared: false,
      target,
      reason: `即梦模型选择失败，生成请求会由拦截器兜底修正: ${String(e?.message || e)}`,
      trigger_selector: '',
    })
  }
})()

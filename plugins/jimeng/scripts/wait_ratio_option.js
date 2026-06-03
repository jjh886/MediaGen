// 等待即梦比例弹窗展开，定位目标比例 radio label 并写入稳定 selector。
;(async () => {
  const SK = Symbol.for('_jimeng')
  const state = window[SK] || {}
  const targetRatio = String(state.params?.ratio || '9:16').trim() || '9:16'
  const markerAttr = 'data-mediagen-jimeng-ratio-option'
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

  const isVisible = (el) => {
    if (!el) return false
    const rect = el.getBoundingClientRect?.()
    if (!rect || rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle?.(el)
    return (
      !style ||
      (style.display !== 'none' &&
        style.visibility !== 'hidden' &&
        style.opacity !== '0' &&
        style.pointerEvents !== 'none')
    )
  }

  const textOf = (el) =>
    [
      el?.textContent,
      el?.getAttribute?.('aria-label'),
      el?.getAttribute?.('title'),
      el?.getAttribute?.('value'),
    ]
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim()

  if (state.ratio_ready === true) {
    return JSON.stringify({
      found: false,
      skipped: true,
      target_text: targetRatio,
      reason: '比例已是目标项',
      selector: '',
    })
  }

  const findByInput = () => {
    const escapedRatio = targetRatio.replace(/\\/g, '\\\\').replace(/"/g, '\\"')
    const input = document.querySelector(
      `.lv-popover-content input[type="radio"][value="${escapedRatio}"], input[type="radio"][value="${escapedRatio}"]`
    )
    const label = input?.closest?.('label')
    return isVisible(label) ? label : null
  }

  const findByText = () => {
    const nodes = [
      ...document.querySelectorAll(
        '.lv-popover-content label, .lv-popover-content [role="radio"], label, [role="radio"]'
      ),
    ].filter(isVisible)
    return nodes.find((node) => textOf(node).includes(targetRatio))
  }

  try {
    document
      .querySelectorAll(`[${markerAttr}="true"]`)
      .forEach((el) => el.removeAttribute(markerAttr))
    for (let i = 0; i < 20; i++) {
      const option = findByInput() || findByText()
      if (option) {
        option.setAttribute(markerAttr, 'true')
        return JSON.stringify({
          found: true,
          target_text: targetRatio,
          option_text: textOf(option),
          selector: `[${markerAttr}="true"]`,
        })
      }
      await sleep(250)
    }
    return JSON.stringify({
      found: false,
      target_text: targetRatio,
      reason: '未找到目标比例选项',
      selector: '',
    })
  } catch (e) {
    return JSON.stringify({
      found: false,
      target_text: targetRatio,
      reason: String(e?.message || e),
      selector: '',
    })
  }
})()

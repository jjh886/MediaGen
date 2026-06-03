// ChatGPT DOM 工具：集中处理可见性、文本读取和 composer 范围定位。
;(() => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || (window[SK] = {})
  if (state.dom) return

  const interactiveSelector = [
    'button',
    '[role="button"]',
    '[role="tab"]',
    '[role="radio"]',
    '[role="menuitem"]',
    '[role="menuitemradio"]',
    '[role="option"]',
    'label',
  ].join(', ')

  const isVisible = (el) => {
    if (!el) return false
    const box = el.getBoundingClientRect()
    if (Math.max(box.width, box.height) <= 0) return false

    const css = window.getComputedStyle(el)
    if (!css) return true
    const hiddenByStyle =
      css.display === 'none' ||
      css.visibility === 'hidden' ||
      css.pointerEvents === 'none' ||
      Number(css.opacity || '1') === 0
    return !hiddenByStyle
  }

  const textOf = (el) => {
    const parts = ['textContent', 'aria-label', 'title', 'data-testid', 'value'].map((key) =>
      key === 'textContent' ? el?.textContent || '' : el?.getAttribute?.(key) || ''
    )
    return parts.join(' ').replace(/\s+/g, ' ').trim().toLowerCase()
  }

  const clearMarker = (markerAttr) => {
    document.querySelectorAll(`[${markerAttr}]`).forEach((el) => el.removeAttribute(markerAttr))
  }

  const collectInteractive = (root) =>
    Array.from(root?.querySelectorAll(interactiveSelector) || [])
      .filter(isVisible)
      .filter((el) => !el.closest('[role="menu"]'))
      .filter((el) => !el.closest('nav, aside'))

  const hasComposerControl = (root) =>
    !!root?.querySelector('#composer-plus-btn') ||
    !!root?.querySelector('input#upload-files[type=file]')

  const hasImageishControl = (root, tokens = []) =>
    collectInteractive(root).some((el) => {
      const text = textOf(el)
      return (
        text.includes('图片') ||
        text.includes('image') ||
        tokens.some((token) => text.includes(String(token).toLowerCase()))
      )
    })

  const fallbackComposerRoot = (textarea) =>
    textarea.closest('form') || textarea.parentElement || document.body || document.documentElement

  const findComposerRoot = (options = {}) => {
    const textarea = document.querySelector('#prompt-textarea')
    if (!textarea) return document.body || document.documentElement

    let best = textarea.parentElement
    let current = textarea.parentElement
    while (current) {
      const hasBaseControls = hasComposerControl(current)
      const hasRequiredImageControls =
        !options.requireImageish || hasImageishControl(current, options.imageTokens || [])
      if (hasBaseControls && hasRequiredImageControls) best = current
      current = current.parentElement
    }

    return best || fallbackComposerRoot(textarea)
  }

  state.dom = {
    clearMarker,
    collectInteractive,
    findComposerRoot,
    isVisible,
    textOf,
  }
})()

;(() => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || {}
  const visible = state.dom?.isVisible || (() => false)
  const params = state.params || {}
  const target = String(params.ratioLabel || '').trim()
  const ratio = String(params.ratio || '').trim()
  const markerAttr = 'data-mediagen-cgpt-ratio-option'
  const menuLabelTokens = ['choose image aspect ratio', 'image aspect ratio', '宽高比', '比例']
  const ratioTokens = [target, ratio].filter(Boolean)
  const allRatioTokens = [
    '9:16',
    '16:9',
    '1:1',
    '3:4',
    '4:3',
    '故事',
    '宽屏',
    '方形',
    '竖版',
    '横屏',
  ]

  const normalize = (text) =>
    String(text || '')
      .replace(/\s+/g, ' ')
      .trim()
  const lower = (text) => normalize(text).toLowerCase()
  const optionText = (el) =>
    normalize([el?.textContent || '', el?.getAttribute('aria-label') || ''].join(' '))

  const matchesTargetRatio = (el) => {
    const text = optionText(el)
    return ratioTokens.some((token) => text.includes(token))
  }

  const looksLikeRatioMenu = (menu) => {
    const label = lower(menu.querySelector('.__menu-label')?.textContent)
    if (menuLabelTokens.some((token) => label.includes(token.toLowerCase()))) return true

    const radios = Array.from(menu.querySelectorAll('[role="menuitemradio"]')).filter(visible)
    const ratioHits = radios.filter((el) => {
      const text = optionText(el)
      return allRatioTokens.some((token) => text.includes(token))
    })
    return ratioHits.length >= 3
  }

  const selectClickToken = (option) => {
    const text = optionText(option)
    return (
      ratioTokens.find((token) => text.includes(token)) || normalize(option.textContent) || ratio
    )
  }

  const start = Date.now()

  return new Promise((resolve) => {
    const timer = setInterval(() => {
      document.querySelectorAll(`[${markerAttr}]`).forEach((el) => el.removeAttribute(markerAttr))
      const menus = Array.from(document.querySelectorAll('[role="menu"]'))
        .filter(visible)
        .filter(looksLikeRatioMenu)

      const menu = menus[menus.length - 1]
      if (menu) {
        const option = Array.from(menu.querySelectorAll('[role="menuitemradio"]'))
          .filter(visible)
          .find(matchesTargetRatio)
        if (option) {
          option.setAttribute(markerAttr, 'true')
          clearInterval(timer)
          resolve(
            JSON.stringify({
              found: true,
              selector: `[${markerAttr}="true"]`,
              target_text: selectClickToken(option),
              target_ratio: ratio,
              option_text: optionText(option),
              w: 1,
            })
          )
          return
        }
      }

      if (Date.now() - start >= 4000) {
        clearInterval(timer)
        resolve(JSON.stringify({ found: false, selector: '', target_text: '', w: 1 }))
      }
    }, 100)
  })
})()

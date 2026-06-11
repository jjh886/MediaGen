;(() => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || {}
  const visible = state.dom?.isVisible || (() => false)
  const params = state.params || {}
  const ratio = String(params.ratio || '').trim()
  const target = String(params.ratioLabel || '').trim()
  const markerAttr = 'data-mediagen-cgpt-ratio-option'
  const menuLabelTokens = ['choose image aspect ratio', 'image aspect ratio', '宽高比', '比例']
  // 2:1 是全景提示词意图，不是 docs/chatgpt 里能证明存在的菜单项；菜单侧最多借 Auto。
  const ratioTokens =
    ratio === '2:1'
      ? ['Auto', 'auto', '自动']
      : ratio === 'auto'
        ? [target, 'auto', '自动'].filter(Boolean)
        : [target, ratio].filter(Boolean)
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
    'auto',
    '自动',
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

  const findOption = (menu, matcher) =>
    Array.from(menu.querySelectorAll('[role="menuitemradio"]')).filter(visible).find(matcher)

  const findStoryFallback = (menu) =>
    findOption(menu, (el) => {
      const text = optionText(el)
      return text.includes('9:16') || text.includes('故事') || lower(text).includes('story')
    })

  const effectivePromptRatio = (fallback) => {
    if (ratio === '2:1') return '2:1'
    if (ratio === 'auto') return fallback === 'story_for_auto' ? '9:16' : 'auto'
    return ratio || '9:16'
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
        const option = findOption(menu, matchesTargetRatio)
        const storyOption = ratio === 'auto' && !option ? findStoryFallback(menu) : null
        const selectedOption = option || storyOption
        const fallback = storyOption
          ? 'story_for_auto'
          : ratio === '2:1' && option
            ? 'auto_for_2_1'
            : ''
        if (selectedOption) {
          selectedOption.setAttribute(markerAttr, 'true')
          clearInterval(timer)
          resolve(
            JSON.stringify({
              found: true,
              selector: `[${markerAttr}="true"]`,
              target_text: selectClickToken(selectedOption),
              target_ratio: ratio,
              effective_ratio: effectivePromptRatio(fallback),
              option_text: optionText(selectedOption),
              fallback,
              w: 1,
            })
          )
          return
        }
      }

      if (Date.now() - start >= 4000) {
        clearInterval(timer)
        resolve(
          JSON.stringify({
            found: false,
            selector: '',
            target_text: '',
            target_ratio: ratio,
            effective_ratio: ratio === 'auto' ? '9:16' : ratio || '9:16',
            fallback: ratio === 'auto' ? 'prompt_story_for_auto' : 'prompt_only',
            w: 1,
          })
        )
      }
    }, 100)
  })
})()

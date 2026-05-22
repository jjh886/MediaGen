// ChatGPT: detect whether the current composer is already in image mode.
// If yes, later "+ -> 创建图片" steps should be skipped.
;(() => {
  const markerAttr = 'data-frame64-cgpt-image-entry'
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
  const imageModeTokens = ['图片', 'image']
  const directEntryTokens = ['生成图片', 'create image', 'image generation']

  const visible = (el) => {
    if (!el) return false
    const rect = el.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle(el)
    if (!style) return true
    if (style.display === 'none') return false
    if (style.visibility === 'hidden') return false
    if (style.pointerEvents === 'none') return false
    return Number(style.opacity || '1') !== 0
  }

  const textOf = (el) =>
    [
      el?.textContent || '',
      el?.getAttribute?.('aria-label') || '',
      el?.getAttribute?.('title') || '',
      el?.getAttribute?.('data-testid') || '',
      el?.getAttribute?.('value') || '',
    ]
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim()
      .toLowerCase()

  const findComposerRoot = () => {
    const textarea = document.querySelector('#prompt-textarea')
    if (!textarea) return document.body || document.documentElement

    let best = textarea.parentElement
    let current = textarea.parentElement
    while (current) {
      const hasPlus = !!current.querySelector('#composer-plus-btn')
      const hasUpload = !!current.querySelector('input#upload-files[type=file]')
      if (hasPlus || hasUpload) best = current
      current = current.parentElement
    }

    return (
      best ||
      textarea.closest('form') ||
      textarea.parentElement ||
      document.body ||
      document.documentElement
    )
  }

  const composerRoot = findComposerRoot()
  document.querySelectorAll(`[${markerAttr}]`).forEach((el) => el.removeAttribute(markerAttr))

  const candidates = Array.from(composerRoot.querySelectorAll(interactiveSelector))
    .filter(visible)
    .filter((el) => !el.closest('[role="menu"]'))
    .filter((el) => !el.closest('nav, aside'))

  const ratioTrigger = candidates.find((el) => {
    const text = textOf(el)
    if (!text) return false
    return triggerTextTokens.some((token) => text === token || text.startsWith(`${token} `))
  })

  const imageChip = candidates.find((el) => {
    const text = textOf(el)
    if (!text) return false
    return imageModeTokens.some((token) => text.includes(token))
  })

  const directEntry = candidates.find((el) => {
    const text = textOf(el)
    if (!text) return false
    if (triggerTextTokens.some((token) => text.includes(token))) return false
    return (
      directEntryTokens.some((token) => text.includes(token.toLowerCase())) ||
      text === '图片' ||
      text.startsWith('图片 ')
    )
  })

  const already = !!(ratioTrigger && imageChip)
  if (!already && directEntry) {
    directEntry.setAttribute(markerAttr, 'true')
  }

  return JSON.stringify({
    already_in_image_mode: already,
    plus_selector: already ? '' : directEntry ? `[${markerAttr}="true"]` : '#composer-plus-btn',
    entry_via: already ? 'already' : directEntry ? 'direct' : 'plus_menu',
    skip_create_image_entry: already || !!directEntry,
    mode_hint: ratioTrigger ? textOf(ratioTrigger) : '',
  })
})()

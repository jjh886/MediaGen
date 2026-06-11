// ChatGPT: detect whether the current composer is already in image mode.
// If yes, later "+ -> 创建图片" steps should be skipped.
;(() => {
  const dom = window[Symbol.for('_cgpt')]?.dom
  const markerAttr = 'data-mediagen-cgpt-image-entry'
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
  const directEntryTokens = [
    '生成图片',
    '创建图片',
    '制作图片',
    'create image',
    'create an image',
    'generate image',
    'image generation',
  ]

  if (!dom) {
    return JSON.stringify({
      already_in_image_mode: false,
      plus_selector: '#composer-plus-btn',
      entry_via: 'helper_missing',
      skip_create_image_entry: false,
      mode_hint: '',
    })
  }

  const composerRoot = dom.findComposerRoot()
  dom.clearMarker(markerAttr)
  const candidates = dom.collectInteractive(composerRoot)

  const ratioTrigger = candidates.find((el) => {
    const text = dom.textOf(el)
    if (!text) return false
    return triggerTextTokens.some((token) => text === token || text.startsWith(`${token} `))
  })

  const imageChip = candidates.find((el) => {
    const text = dom.textOf(el)
    if (!text) return false
    return imageModeTokens.some((token) => text.includes(token))
  })

  const directEntry = candidates.find((el) => {
    const text = dom.textOf(el)
    if (!text) return false
    if (triggerTextTokens.some((token) => text.includes(token))) return false
    return (
      directEntryTokens.some((token) => text.includes(token.toLowerCase())) ||
      text === '图片' ||
      text === 'image' ||
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
    mode_hint: ratioTrigger ? dom.textOf(ratioTrigger) : '',
  })
})()

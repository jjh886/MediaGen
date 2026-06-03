// 等待 ChatGPT composer 真正可提交，并标记发送按钮供 CDP 真实点击。
;(async () => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || {}
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
  const expectedRefs = Number(state?.params?.refCount || 0)
  const markerAttr = 'data-mediagen-cgpt-send-button'

  const norm = (text) =>
    String(text || '')
      .replace(/\s+/g, ' ')
      .trim()
      .toLowerCase()

  const isVisible = (el) => {
    if (!el) return false
    const rect = el.getBoundingClientRect?.()
    if (!rect || rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle?.(el)
    return (
      !style || (style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0')
    )
  }

  const findComposerRoot = () => {
    const textarea = document.querySelector('#prompt-textarea[contenteditable="true"]')
    return (
      textarea?.closest('form') ||
      textarea?.parentElement ||
      document.body ||
      document.documentElement
    )
  }

  const attachmentCardCount = (root) =>
    root.querySelectorAll(
      [
        '[aria-label="以全视图打开图片"]',
        '[aria-label="Open image in full view"]',
        'img[src^="blob:"]',
        'img[src^="data:image"]',
      ].join(', ')
    ).length

  const isUsableButton = (button) => {
    if (!button || !isVisible(button)) return false
    if (button.disabled || button.getAttribute('aria-disabled') === 'true') return false
    const label = norm(
      [
        button.getAttribute('aria-label'),
        button.getAttribute('data-testid'),
        button.innerText,
        button.textContent,
      ].join(' ')
    )
    if (/(stop|停止|cancel|取消|voice|microphone|麦克风|附件|upload|plus)/.test(label)) return false
    return true
  }

  const findSendButton = (root) => {
    const preferred = [
      'button[data-testid="send-button"]',
      'button[data-testid="composer-send-button"]',
      'button[aria-label*="Send"]',
      'button[aria-label*="发送"]',
      'button[aria-label*="提交"]',
      'button[type="submit"]',
    ]
    for (const selector of preferred) {
      const found = [
        ...root.querySelectorAll(selector),
        ...document.querySelectorAll(selector),
      ].find(isUsableButton)
      if (found) return found
    }

    const buttons = [...root.querySelectorAll('button')]
    return buttons.find((button) => {
      if (!isUsableButton(button)) return false
      const label = norm(
        [
          button.getAttribute('aria-label'),
          button.getAttribute('data-testid'),
          button.innerText,
          button.textContent,
        ].join(' ')
      )
      return /(send|发送|提交|arrow_upward|up-arrow|submit)/.test(label)
    })
  }

  try {
    document.querySelectorAll(`[${markerAttr}="true"]`).forEach((el) => {
      el.removeAttribute(markerAttr)
    })

    const deadline = Date.now() + 60000
    let cardsReadyAt = 0
    let last = { uploaded: 0, cards: 0, button_found: false }

    while (Date.now() < deadline) {
      const root = findComposerRoot()
      const uploaded = Array.isArray(state.uploaded_refs) ? state.uploaded_refs.length : 0
      const cards = attachmentCardCount(root)
      if (expectedRefs > 0 && cards >= expectedRefs) {
        cardsReadyAt = cardsReadyAt || Date.now()
      } else {
        cardsReadyAt = 0
      }

      const refsReady =
        expectedRefs <= 0 ||
        uploaded >= expectedRefs ||
        (cardsReadyAt > 0 && Date.now() - cardsReadyAt >= 3000)
      const button = findSendButton(root)
      last = { uploaded, cards, button_found: !!button }

      if (refsReady && button) {
        button.setAttribute(markerAttr, 'true')
        return JSON.stringify({
          ready: true,
          selector: `[${markerAttr}="true"]`,
          expectedRefs,
          uploaded,
          cards,
          confirmedBy: uploaded >= expectedRefs ? 'upload_process_stream' : 'dom_cards_stable',
        })
      }

      await sleep(500)
    }

    const message =
      `生成过于频繁: 发送按钮未就绪，请稍等几分钟后再重试。` +
      `expectedRefs=${expectedRefs}, uploaded=${last.uploaded}, ` +
      `cards=${last.cards}, button_found=${last.button_found}`

    return JSON.stringify({
      error: message,
      error_code: 'rate_limit_exceeded',
    })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()

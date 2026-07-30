// ChatGPT 页面准备
// 目标：
// 1. 尽早识别登录失效；
// 2. 若落在旧会话页面，回到新聊天页面；
// 3. 等待 composer ready。
;(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const composerSelectors = [
    '#prompt-textarea[contenteditable="true"]',
    '[role="textbox"][contenteditable="true"]',
    '[contenteditable="true"][data-lexical-editor="true"]',
    '[contenteditable="true"]',
    'textarea',
  ]
  // composer 可能没有 #prompt-textarea，用运行时标记为后续 CDP 输入提供稳定选择器。
  const composerMarker = 'data-mediagen-cgpt-composer'
  const composerSelector = `[${composerMarker}="true"]`

  function isVisible(el) {
    if (!el) return false
    const rect = el.getBoundingClientRect?.()
    if (!rect || rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle?.(el)
    return (
      !style || (style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0')
    )
  }

  function findComposer() {
    for (const selector of composerSelectors) {
      const found = Array.from(document.querySelectorAll(selector)).find((el) => {
        if (el.closest?.('nav, aside')) return false
        return isVisible(el)
      })
      if (found) return found
    }
    return null
  }

  function markComposer(composer) {
    document.querySelectorAll(`[${composerMarker}]`).forEach((el) => {
      el.removeAttribute(composerMarker)
    })
    composer.setAttribute(composerMarker, 'true')
    return composerSelector
  }

  function snapshot() {
    const composer = findComposer()
    const uploadInput = document.querySelector('input#upload-files[type=file]')
    return {
      url: location.href,
      title: document.title,
      readyState: document.readyState,
      promptCount: document.querySelectorAll('#prompt-textarea[contenteditable="true"]').length,
      anyComposer: !!composer,
      upload_ready: !!uploadInput,
      fileInputs: document.querySelectorAll('input#upload-files[type=file]').length,
      plusButtons: document.querySelectorAll('#composer-plus-btn').length,
    }
  }

  try {
    const bodyText = document.body?.innerText || ''

    if (
      bodyText.includes('Log in') ||
      bodyText.includes('登录') ||
      bodyText.includes('Continue with') ||
      bodyText.includes('继续使用')
    ) {
      return JSON.stringify({
        error: '登录已失效，请重新登录 ChatGPT',
        login_expired: true,
        ...snapshot(),
      })
    }

    // 已在历史会话时，点击 New chat，避免把结果串进旧会话。
    if (location.pathname.startsWith('/c/')) {
      const newChat = document.querySelector(
        '[data-testid="create-new-chat-button"], a[href="https://chatgpt.com/"], a[href="/"]'
      )
      if (newChat) {
        newChat.click()
        await sleep(1500)
      }
    }

    for (let i = 0; i < 30; i++) {
      const composer = findComposer()
      // 上传 input 可能延后挂载；composer 可用就可以继续后续步骤。
      if (composer) {
        composer.focus()
        return JSON.stringify({
          prepared: true,
          composer_selector: markComposer(composer),
          ...snapshot(),
        })
      }
      await sleep(500)
    }

    return JSON.stringify({
      error: 'ChatGPT composer 未就绪',
      ...snapshot(),
    })
  } catch (e) {
    return JSON.stringify({
      error: String(e?.message || e),
      ...snapshot(),
    })
  }
})()

// ChatGPT 页面准备
// 目标：
// 1. 尽早识别登录失效；
// 2. 若落在旧会话页面，回到新聊天页面；
// 3. 等待 composer ready。
;(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

  function snapshot() {
    return {
      url: location.href,
      title: document.title,
      readyState: document.readyState,
      promptCount: document.querySelectorAll('#prompt-textarea[contenteditable="true"]').length,
      fileInputs: document.querySelectorAll('input#upload-files[type=file]').length,
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
      const composer = document.querySelector('#prompt-textarea[contenteditable="true"]')
      const fileInput = document.querySelector('input#upload-files[type=file]')
      if (composer && fileInput) {
        composer.focus()
        return JSON.stringify({ prepared: true, ...snapshot() })
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

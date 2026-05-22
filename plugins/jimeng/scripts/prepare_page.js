// 等待即梦图片生成页的核心控件出现。
;(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

  const isVisible = (el) => {
    if (!el) return false
    const rect = el.getBoundingClientRect?.()
    if (!rect || rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle?.(el)
    return (
      !style || (style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0')
    )
  }

  const closeFirstPopup = () => {
    const roots = [
      ...document.querySelectorAll(
        '[role="dialog"], [class*="modal"], [class*="dialog"], [class*="popover"]'
      ),
    ].filter(isVisible)
    for (const root of roots) {
      const close = [...root.querySelectorAll('button, [role="button"], [aria-label="关闭"]')].find(
        (node) => {
          const text = (node.textContent || node.getAttribute?.('aria-label') || '').trim()
          return isVisible(node) && /关闭|知道了|我知道了|明白了|确定|OK/i.test(text)
        }
      )
      if (close) {
        close.click()
        return true
      }
    }
    return false
  }

  const findEditor = () =>
    [
      ...document.querySelectorAll(
        '.ProseMirror[contenteditable="true"], div[role="textbox"][contenteditable="true"]'
      ),
    ].find(isVisible)

  const findSubmitButton = () =>
    [
      ...document.querySelectorAll(
        [
          '[class*="toolbar-actions"] button[class*="submit-button"]',
          '[class*="collapsed-submit-button-container"] button[class*="submit-button"]',
          'button[class*="submit-button"]',
        ].join(', ')
      ),
    ].find(isVisible)

  try {
    closeFirstPopup()
    const deadline = Date.now() + 45000
    let last = {}
    while (Date.now() < deadline) {
      const editor = findEditor()
      const input = document.querySelector('input[type="file"][accept*="image"]')
      const submit = findSubmitButton()
      last = {
        url: location.href,
        title: document.title,
        editor: !!editor,
        fileInput: !!input,
        submit: !!submit,
        bodyHint: (document.body?.innerText || '').slice(0, 80),
      }
      if (editor && input) {
        return JSON.stringify({ prepared: true, ...last })
      }
      await sleep(500)
    }
    return JSON.stringify({ error: '即梦图片生成页加载超时', snapshot: last })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()

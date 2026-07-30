// ChatGPT：通过 paste 事件把参考图注入 composer，沿用 Flow 的 data URL 路径。
//
// 直接设置隐藏 file input 时，ChatGPT 的 React 上传链路偶发不会完整触发，
// 页面看起来有文件但发送按钮一直不可用。本脚本模拟真实粘贴：
//   1. 将 manifest 注入的 data URL 转成 File；
//   2. 串行 dispatch paste，避免多图上传顺序抖动；
//   3. 每张图等待拦截器 uploaded_refs 或 DOM 附件卡片确认；
//   4. 返回 pasted/total/uploaded/cards/details 供日志诊断。
;(async () => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || {}
  const params = state.params || {}
  const refs = Array.isArray(params.refImageDataUrls) ? params.refImageDataUrls : []
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

  const getComposer = () =>
    document.querySelector('[data-mediagen-cgpt-composer="true"]') ||
    document.querySelector('#prompt-textarea[contenteditable="true"]') ||
    document.querySelector('[role="textbox"][contenteditable="true"]') ||
    document.querySelector('[contenteditable="true"]') ||
    document.querySelector('textarea')

  const dataUrlToBlob = (url) => {
    const match = String(url || '').match(/^data:([^;,]+)?(;base64)?,([\s\S]*)$/)
    if (!match) throw new Error('参考图不是 data URL')
    const mime = match[1] || 'image/png'
    const isBase64 = !!match[2]
    const raw = isBase64 ? atob(match[3]) : decodeURIComponent(match[3])
    const bytes = new Uint8Array(raw.length)
    for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i)
    return new Blob([bytes], { type: mime })
  }

  const dataUrlToFile = (url, index) => {
    const blob = dataUrlToBlob(url)
    const mime = blob.type || 'image/png'
    const ext = mime.includes('jpeg') ? 'jpg' : mime.includes('webp') ? 'webp' : 'png'
    return new File([blob], `chatgpt_ref_${index}_${Date.now()}.${ext}`, { type: mime })
  }

  const makePasteEvent = (file) => {
    const dt = new DataTransfer()
    dt.items.add(file)
    let event = new ClipboardEvent('paste', {
      bubbles: true,
      cancelable: true,
      clipboardData: dt,
    })
    // main world 下 clipboardData 可能被浏览器置空，降级为自定义 Event。
    if (!event.clipboardData || event.clipboardData !== dt) {
      event = new Event('paste', { bubbles: true, cancelable: true })
      Object.defineProperty(event, 'clipboardData', { value: dt, writable: false })
    }
    return event
  }

  // 与最终上传校验使用同一组 DOM 证据；这里只复用选择器结果，不复制整段计数流程。
  const attachmentSelector = [
    '[aria-label="以全视图打开图片"]',
    '[aria-label="Open image in full view"]',
    '[aria-label*="移除附件"]',
    '[aria-label*="Remove attachment"]',
    '[aria-label*="Remove file"]',
    'img[src^="blob:"]',
    'img[src^="data:image"]',
  ].join(', ')
  const visibleAttachmentCount = () => document.querySelectorAll(attachmentSelector).length

  const waitOneAccepted = async (targetCount) => {
    // 优先信任上传拦截器；拦截器没回包时用 DOM 附件卡兜底。
    const deadline = Date.now() + 25000
    while (Date.now() < deadline) {
      const uploaded = Array.isArray(state.uploaded_refs) ? state.uploaded_refs.length : 0
      if (uploaded >= targetCount || visibleAttachmentCount() >= targetCount) return true
      await sleep(250)
    }
    return false
  }

  try {
    if (!refs.length) {
      return JSON.stringify({ skipped: true, reason: '无参考图', pasted: 0, total: 0 })
    }

    const composer = getComposer()
    if (!composer) return JSON.stringify({ error: '未找到 ChatGPT composer' })
    composer.focus()
    if (typeof composer.click === 'function') composer.click()
    await sleep(200)

    const details = []
    let pasted = 0
    for (let i = 0; i < refs.length; i++) {
      try {
        const file = dataUrlToFile(refs[i], i)
        composer.dispatchEvent(makePasteEvent(file))
        pasted += 1
        const accepted = await waitOneAccepted(i + 1)
        details.push({ index: i, pasted: true, accepted })
      } catch (error) {
        details.push({ index: i, error: String(error?.message || error) })
      }
      await sleep(300)
    }

    return JSON.stringify({
      pasted,
      total: refs.length,
      uploaded: Array.isArray(state.uploaded_refs) ? state.uploaded_refs.length : 0,
      cards: visibleAttachmentCount(),
      details,
    })
  } catch (error) {
    return JSON.stringify({ error: String(error?.message || error) })
  }
})()

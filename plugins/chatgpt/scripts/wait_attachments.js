// 等待 ChatGPT 参考图上传完成
// 主信号：setup_interceptor.js 记录的 uploaded_refs 数量
// 兜底：composer 上显示图片卡片
;(async () => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || {}
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

  const expected = Number(state?.params?.refCount || 0)
  if (!expected) {
    return JSON.stringify({ skipped: true, reason: '无参考图' })
  }

  const visible = (el) => {
    if (!el) return false
    const rect = el.getBoundingClientRect?.()
    if (rect && rect.width <= 0 && rect.height <= 0) return false
    const style = window.getComputedStyle?.(el)
    return !style || (style.display !== 'none' && style.visibility !== 'hidden')
  }

  const countVisibleCards = () =>
    document.querySelectorAll(
      [
        '[aria-label="以全视图打开图片"]',
        '[aria-label="Open image in full view"]',
        '[aria-label*="移除附件"]',
        '[aria-label*="Remove attachment"]',
        '[aria-label*="Remove file"]',
        'img[src^="blob:"]',
        'img[src^="data:image"]',
      ].join(', ')
    ).length

  const inputFileCount = () => {
    const input = document.querySelector('input#upload-files[type=file]')
    return Number(input?.files?.length || 0)
  }

  try {
    const deadline = Date.now() + 75000
    let lastInputFiles = 0
    while (Date.now() < deadline) {
      const uploaded = Array.isArray(state.uploaded_refs) ? state.uploaded_refs.length : 0
      if (uploaded >= expected) {
        return JSON.stringify({
          uploaded,
          expected,
          confirmedBy: 'upload_process_stream',
        })
      }

      const cards = [...document.querySelectorAll('[aria-label], img')].filter(visible).length
        ? countVisibleCards()
        : 0
      if (cards >= expected) {
        return JSON.stringify({
          uploaded,
          expected,
          cards,
          confirmedBy: 'dom_cards',
        })
      }

      lastInputFiles = inputFileCount()

      await sleep(500)
    }

    return JSON.stringify({
      error: `参考图上传超时，expected=${expected}, uploaded=${state.uploaded_refs?.length || 0}, inputFiles=${lastInputFiles}`,
    })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()

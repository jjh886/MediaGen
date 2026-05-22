// 等待即梦参考图上传完成。优先信任接口拦截到的 uri，降级看 DOM 参考图卡片稳定。
;(async () => {
  const SK = Symbol.for('_jimeng')
  const state = window[SK] || {}
  const expectedRefs = Number(state.params?.refCount || 0)
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

  const visibleRect = (el) => {
    const rect = el?.getBoundingClientRect?.()
    return rect && rect.width > 0 && rect.height > 0
  }

  const cardCount = () => {
    const uploadRoot =
      document.querySelector('[class*="reference"], [id*="reference-upload"]')?.parentElement ||
      document.body
    const imgs = uploadRoot.querySelectorAll(
      'img[src^="blob:"], img[src^="data:image"], img[src*="dreamina-sign"], canvas'
    )
    return [...imgs].filter(visibleRect).length
  }

  try {
    if (expectedRefs <= 0) {
      return JSON.stringify({ skipped: true, reason: '无参考图', expectedRefs: 0 })
    }

    const deadline = Date.now() + 90000
    let cardsReadyAt = 0
    let last = { uploaded: 0, cards: 0 }
    while (Date.now() < deadline) {
      const uploaded = Array.isArray(state.uploaded_refs) ? state.uploaded_refs.length : 0
      const cards = cardCount()
      last = { uploaded, cards }

      if (uploaded >= expectedRefs) {
        state.uploads_ready = true
        state.uploads_ready_source = 'upload_api'
        state.uploaded_dom_cards = cards
        return JSON.stringify({
          ready: true,
          expectedRefs,
          uploaded,
          cards,
          confirmedBy: 'upload_api',
        })
      }

      if (cards >= expectedRefs) {
        cardsReadyAt = cardsReadyAt || Date.now()
        if (Date.now() - cardsReadyAt >= 4000) {
          state.uploads_ready = true
          state.uploads_ready_source = 'dom_cards_stable'
          state.uploaded_dom_cards = cards
          return JSON.stringify({
            ready: true,
            expectedRefs,
            uploaded,
            cards,
            confirmedBy: 'dom_cards_stable',
          })
        }
      } else {
        cardsReadyAt = 0
      }
      await sleep(500)
    }
    return JSON.stringify({
      error: `即梦参考图上传等待超时 expected=${expectedRefs}, uploaded=${last.uploaded}, cards=${last.cards}`,
    })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()

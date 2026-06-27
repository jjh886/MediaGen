// 豆包图片生成轮询
// 优先从 SSE 拦截器获取结果（state.status/images）
// 降级：DOM 扫描 rc_gen_image 数量稳定
;(() => {
  const SK = Symbol.for('_s')
  const state = window[SK] || { status: 'generating', startTime: Date.now() }

  // SSE 拦截器已完成
  if (state.status === 'done' && state.images && state.images.length > 0) {
    return JSON.stringify({
      status: 'done',
      imageCount: state.images.length,
      images: state.images,
      source: 'sse_interceptor',
    })
  }

  if (state.status === 'error') {
    return JSON.stringify({
      status: 'error',
      message: state.message,
      error_code: state.error_code,
    })
  }

  // 需要人工验证：返回特殊状态，由 Rust 层显示浏览器窗口
  if (state.status === 'needs_verification') {
    return JSON.stringify({
      status: 'needs_verification',
      message: state.message || 'rate limited, needs verification',
      error_code: state.error_code,
    })
  }

  // SSE 部分完成：有图片但未达到预期数量
  if (state._completedCount > 0 && state.expectedCount > 0) {
    // 数量停滞超过 30 秒，视为完成（部分图片可能生成失败）
    if (state._completedCount === (state._lastPartialCount || 0)) {
      state._partialStallMs = (state._partialStallMs || 0) + 3000
    } else {
      state._partialStallMs = 0
      state._lastPartialCount = state._completedCount
    }
    window[SK] = state

    if (state._partialStallMs >= 30000) {
      state.status = 'done'
      return JSON.stringify({
        status: 'done',
        imageCount: state.images.length,
        images: state.images,
        source: 'sse_partial_stable',
      })
    }

    return JSON.stringify({
      status: 'generating',
      completed: state._completedCount,
      expected: state.expectedCount,
      stall: state._partialStallMs,
      source: 'sse_partial',
    })
  }

  // 降级：DOM 扫描（SSE 拦截失败时的 fallback）
  const allImgs = document.querySelectorAll('img[src*="rc_gen_image"]')
  let newRcCount = 0
  for (const img of allImgs) {
    if (img.naturalWidth > 50) newRcCount++
  }

  if (newRcCount > 0) {
    if (newRcCount === (state._lastRcCount || 0)) {
      state._stableCount = (state._stableCount || 0) + 1
    } else {
      state._stableCount = 0
    }
    state._lastRcCount = newRcCount
    window[SK] = state

    // 稳定 3 轮（9秒）视为完成 → 提取 DOM 中的图片 URL
    if (state._stableCount >= 3) {
      const domImages = []
      for (const img of allImgs) {
        if (img.naturalWidth > 50 && img.src) {
          const src = img.src
          domImages.push({
            url: src,
            width: img.naturalWidth || 0,
            height: img.naturalHeight || 0,
          })
        }
      }
      return JSON.stringify({
        status: 'done',
        imageCount: newRcCount,
        images: domImages,
        source: 'dom_scan_stable',
      })
    }
  }

  const elapsed = Date.now() - (state.startTime || Date.now())
  const bodyText = document.body?.innerText || ''

  // 内容违规检测（立即生效，不等 15 秒，因为违规响应通常秒回）
  const fatalPhrases = ['无法生成', '违反', '不符合', '内容政策', '尝试输入其他']
  for (const phrase of fatalPhrases) {
    if (bodyText.includes(phrase)) {
      return JSON.stringify({
        status: 'error',
        error_code: 'content_policy_refusal',
        message: `内容违规: "${phrase}"`,
        retryable: false,
        usage_consumed: false,
      })
    }
  }

  // 平台繁忙检测（等待 15 秒后，避免误判加载中的文案）
  if (elapsed > 15000) {
    const retryablePhrases = ['有点热闹', '稍后再试', '服务繁忙', '请求频繁', '处理这个任务']
    for (const phrase of retryablePhrases) {
      if (bodyText.includes(phrase)) {
        return JSON.stringify({
          status: 'error',
          error_code: phrase.includes('请求频繁') ? 'rate_limit_exceeded' : 'platform_busy',
          message: `平台受限: "${phrase}"`,
          retryable: true,
          usage_consumed: false,
        })
      }
    }
  }

  // 超时（10 分钟）
  if (elapsed > 600000) {
    return JSON.stringify({
      status: 'error',
      error_code: 'generation_timeout',
      message: '生成超时（10min）',
      retryable: true,
      usage_consumed: false,
    })
  }

  // 轮询中间状态
  const fetchLog = state.fetchLog || []
  const chatReqs = fetchLog.filter((f) => f.url.includes('chat/completion'))
  return JSON.stringify({
    status: 'generating',
    elapsed: Math.round(elapsed / 1000),
    fetchHooked: !!state.fetchHooked,
    chatCompletionFetches: chatReqs.length,
  })
})()

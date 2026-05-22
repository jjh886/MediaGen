// Flow 图片生成轮询脚本
// 优先级：fetch/XHR 拦截器 > DOM 扫描
// 超时 600 秒（与豆包一致）
;(() => {
  const state = window.__frame64 || { status: 'generating', startTime: Date.now() }

  // 拦截器已捕获结果（fifeUrl 直链）
  if (state.status === 'done' && state.url) {
    return JSON.stringify(state)
  }
  if (state.status === 'error') {
    return JSON.stringify(state)
  }

  // DOM 扫描：检查是否有新的生成图片
  const generatedImgs = document.querySelectorAll(
    'img[alt="生成的图片"], img[alt="Generated image"], img[src*="getMediaUrlRedirect"]'
  )
  const baseline = state._initialImgCount || 0
  const allImgBaseline = state._initialAllImgCount || 0

  if (generatedImgs.length > baseline) {
    // 有新图片！取最新的
    const newImg = generatedImgs[generatedImgs.length - 1]
    if (newImg.src && newImg.naturalWidth > 10) {
      // 从 URL 提取 media name（用于后续清理）
      const nameMatch = newImg.src.match(/name=([a-f0-9-]+)/)
      const result = {
        status: 'done',
        url: newImg.src,
        name: nameMatch ? nameMatch[1] : '',
        width: newImg.naturalWidth || 0,
        height: newImg.naturalHeight || 0,
        source: 'dom_scan',
      }
      window.__frame64 = { ...state, ...result }
      return JSON.stringify(result)
    }
  }

  const elapsed = Date.now() - (state.startTime || Date.now())

  // 内容审核检测（立即生效）
  const bodyText = document.body?.innerText || ''
  const fatalPhrases = ['UNSAFE', 'unsafe', '不安全', '违规', '无法生成']
  for (const phrase of fatalPhrases) {
    if (bodyText.includes(phrase)) {
      const result = { status: 'error', message: `内容审核拦截: "${phrase}"`, retryable: false }
      window.__frame64 = result
      return JSON.stringify(result)
    }
  }

  // 平台繁忙检测（等待 15 秒后，避免误判）
  if (elapsed > 15000) {
    const retryablePhrases = ['稍后再试', '服务繁忙', '请求频繁', 'rate limit', 'too many']
    for (const phrase of retryablePhrases) {
      if (bodyText.toLowerCase().includes(phrase.toLowerCase())) {
        const result = { status: 'error', message: `平台受限: "${phrase}"`, retryable: true }
        window.__frame64 = result
        return JSON.stringify(result)
      }
    }
  }

  // 超时（600 秒）
  if (elapsed > 600000) {
    const result = { status: 'error', message: '生成超时（600s）' }
    window.__frame64 = result
    return JSON.stringify(result)
  }

  // 诊断信息
  const fetchLog = window.__frame64_fetch_log || []
  const batchReqs = fetchLog.filter((f) => f.url?.includes('batchGenerate'))
  return JSON.stringify({
    status: 'generating',
    elapsed: Math.round(elapsed / 1000),
    fetchHooked: !!window.__frame64_fetch_hooked,
    totalFetches: fetchLog.length,
    batchGenerateFetches: batchReqs.length,
    currentImgs: generatedImgs.length,
    baseline,
  })
})()

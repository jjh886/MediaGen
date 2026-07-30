// ChatGPT 图片生成轮询
// 优先级：
// 1. setup_interceptor.js 已拿到 download_url
// 2. 从 fetchLog 中发现结果下载接口，再主动拉一次
// 3. conversation_id + DOM 状态兜底
;(async () => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || { status: 'generating', startTime: Date.now() }
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const asAbsoluteUrl = (url) => {
    try {
      return new URL(url, location.origin).href
    } catch (_) {
      return url
    }
  }
  const rememberDone = (downloadUrl, fileId, source) => {
    state.generated_file_id = fileId || state.generated_file_id || ''
    state.download_url = asAbsoluteUrl(downloadUrl)
    state.status = 'done'
    return JSON.stringify({
      status: 'done',
      conversation_id: state.conversation_id || '',
      file_id: state.generated_file_id || '',
      download_url: state.download_url,
      source,
    })
  }
  const collectFileIds = (value, out = new Set(), depth = 0) => {
    if (depth > 12 || value == null) return out
    if (typeof value === 'string') {
      for (const match of value.matchAll(/file_[a-zA-Z0-9]+/g)) out.add(match[0])
      return out
    }
    if (Array.isArray(value)) {
      for (const item of value) collectFileIds(item, out, depth + 1)
      return out
    }
    if (typeof value === 'object') {
      for (const item of Object.values(value)) collectFileIds(item, out, depth + 1)
    }
    return out
  }
  const fetchDownloadForFile = async (fileId, source) => {
    if (!fileId || !state.conversation_id) return null
    const url = `/backend-api/files/download/${fileId}?conversation_id=${encodeURIComponent(
      state.conversation_id
    )}&inline=false`
    try {
      const resp = await fetch(url, { credentials: 'include' })
      const data = await resp.json()
      if (data?.download_url) return rememberDone(data.download_url, fileId, source)
    } catch (_) {}
    return null
  }
  const extractFileId = (value) => (String(value || '').match(/file_[a-zA-Z0-9]+/) || [])[0] || ''
  const collectMessageAttachmentIds = (message) =>
    (message?.metadata?.attachments || []).map((item) => item?.id).filter(Boolean)

  const collectPartAttachmentIds = (message) =>
    (message?.content?.parts || [])
      .filter((part) => typeof part !== 'string')
      .map((part) => extractFileId(part?.asset_pointer || part?.metadata?.asset_pointer))
      .filter(Boolean)

  const collectUserAttachmentIds = (conv) => {
    const ids = new Set()
    const messages = Object.values(conv?.mapping || {})
      .map((node) => node?.message)
      .filter((message) => message?.author?.role === 'user')

    messages
      .flatMap((message) => [
        ...collectMessageAttachmentIds(message),
        ...collectPartAttachmentIds(message),
      ])
      .forEach((id) => ids.add(id))
    return ids
  }
  const rememberConversationTask = (conv) => {
    const message = conv?.mapping?.[conv?.current_node || '']?.message
    state.image_gen_task_id = message?.metadata?.image_gen_task_id || state.image_gen_task_id
  }

  const updateStreamStatus = async (imageCardCount) => {
    if (!state.conversation_id) return
    try {
      const resp = await fetch(`/backend-api/conversation/${state.conversation_id}/stream_status`, {
        credentials: 'include',
      })
      const data = await resp.json()
      if (data?.status && data.status !== 'IS_STREAMING' && imageCardCount === 0) {
        state.stream_status = data.status
      }
    } catch (_) {}
  }

  const probeConversationFiles = async () => {
    // 拦截器漏掉 files/download 时，轻量拉取当前 conversation 找新 file_id。
    if (!state.conversation_id) return null
    try {
      const resp = await fetch(`/backend-api/conversation/${state.conversation_id}`, {
        credentials: 'include',
      })
      const conv = await resp.json()
      state.async_status = conv?.async_status
      rememberConversationTask(conv)

      const uploadedRefs = new Set([
        ...(state.uploaded_refs || []),
        ...collectUserAttachmentIds(conv),
      ])
      const fileIds = [...collectFileIds(conv)].filter((fileId) => !uploadedRefs.has(fileId))
      for (const fileId of fileIds.reverse()) {
        const done = await fetchDownloadForFile(fileId, 'conversation_file_id')
        if (done) return done
      }
    } catch (_) {}
    return null
  }

  const matchAnyPattern = (text, patterns) =>
    patterns.map((pattern) => text.match(pattern)).find(Boolean)?.[0] || ''

  const elapsed = Date.now() - (state.startTime || Date.now())
  const hasFreshDoneResult =
    state.status === 'done' &&
    !!state.download_url &&
    Number(state.submit_started_at || 0) > 0 &&
    Number(state.result_at || 0) >= Number(state.submit_started_at || 0) &&
    (!!state.result_conversation_id || !!state.conversation_id)

  if (hasFreshDoneResult) {
    return rememberDone(state.download_url, state.generated_file_id || '', 'interceptor')
  }

  if (state.status === 'error') {
    return JSON.stringify({
      status: 'error',
      error_code: state.error_code || '',
      message: state.message || '生成失败',
      conversation_id: state.conversation_id || '',
      usage_consumed: false,
    })
  }

  const bodyText = document.body?.innerText || document.body?.textContent || ''
  const dialogTexts = [...document.querySelectorAll('[role="dialog"], [aria-modal="true"]')]
    .map((node) => node.innerText || node.textContent || '')
    .filter(Boolean)
  const rateLimitSources = [...dialogTexts, bodyText].filter(Boolean)
  const dismissRateLimitModal = () => {
    const buttons = [...document.querySelectorAll('button')]
    const target = buttons.find((button) => {
      const text = (button.innerText || button.textContent || '').replace(/\s+/g, '').trim()
      return ['明白了', '知道了', '我知道了', '好的', '确定', 'OK'].includes(text)
    })
    if (target) target.click()
  }
  const rateLimitPatterns = [
    /you['’]re generating images too quickly\.[\s\S]*?please wait for \d+\s+minutes? before generating more images\.?/i,
    /请求过于频繁[\s\S]*?请(?:稍等|等待)(?:\d+|几)\s*分钟后再(?:重试|生成|尝试)。?/i,
    /请(?:稍等|等待)(?:\d+|几)\s*分钟后再(?:重试|生成|尝试)。?/i,
    /you['’]re generating images too quickly/i,
    /please wait for \d+\s+minutes? before generating more images\.?/i,
    /rate_limit_exceeded/i,
    /请求过于频繁/i,
    /暂时限制你访问对话记录/i,
  ]
  for (const sourceText of rateLimitSources) {
    for (const pattern of rateLimitPatterns) {
      const match = sourceText.match(pattern)
      if (match) {
        dismissRateLimitModal()
        const message = (match[0] || '').replace(/\s+/g, ' ').trim().slice(0, 220)
        return JSON.stringify({
          status: 'error',
          error_code: 'rate_limit_exceeded',
          message: `生成过于频繁: ${message || 'rate_limit_exceeded'}`,
          conversation_id: state.conversation_id || '',
          usage_consumed: false,
        })
      }
    }
  }

  const quotaPatterns = [
    /you['’]ve hit the free plan limit for image generations requests\./i,
    /you can create more images when the limit resets in[^.]*\./i,
    /you['’]ve hit the free plan limit for image generations requests/i,
    /you can create more images when the limit resets in/i,
    /free plan limit/i,
    /limit resets in/i,
  ]
  for (const pattern of quotaPatterns) {
    const match = bodyText.match(pattern)
    if (match) {
      const message = (match[0] || '').replace(/\s+/g, ' ').trim().slice(0, 220)
      return JSON.stringify({
        status: 'error',
        error_code: 'quota_exhausted',
        message: `账号额度耗尽: ${message || 'free plan limit'}`,
        conversation_id: state.conversation_id || '',
        usage_consumed: false,
      })
    }
  }

  const refusalPatterns = [
    /我不能帮你(?:生成|描述|创作)[\s\S]{0,120}(?:杀人|抢劫|犯罪|暴力|伤害|不当)/,
    /不能帮你(?:生成|描述|创作)[\s\S]{0,120}(?:犯罪|暴力|伤害|不当)/,
    /不会涉及具体作案细节[\s\S]{0,80}(?:生成画面|画面|内容)?/,
    /不出现暴力伤害细节/,
    /不当画面生成请求/,
    /I can(?:not|['’]t) help (?:generate|create|describe)[\s\S]{0,120}(?:violent|criminal|harmful)/i,
  ]
  const refusalMessage = matchAnyPattern(bodyText, refusalPatterns)
  if (refusalMessage) {
    return JSON.stringify({
      status: 'error',
      error_code: 'content_policy_refusal',
      message: `ChatGPT 拒绝生成图片: ${refusalMessage.replace(/\s+/g, ' ').trim().slice(0, 220)}`,
      conversation_id: state.conversation_id || '',
      usage_consumed: false,
    })
  }

  const fatalPhrases = ['Something went wrong', '无法生成', '错误', 'try again', '稍后重试', '违反']
  for (const phrase of fatalPhrases) {
    if (bodyText.includes(phrase)) {
      return JSON.stringify({
        status: 'error',
        error_code: phrase.includes('违反') ? 'content_policy_refusal' : 'platform_busy',
        message: `页面提示失败: "${phrase}"`,
        conversation_id: state.conversation_id || '',
        retryable: phrase !== '违反',
        usage_consumed: false,
      })
    }
  }

  // 结果接口通常长这样：
  // /backend-api/files/download/file_xxx?conversation_id=...&inline=false
  const resultReq = (state.fetchLog || [])
    .map((item) => item.url || '')
    .reverse()
    .find((url) => {
      if (!url.includes('/backend-api/files/download/') || !url.includes('conversation_id='))
        return false
      if (!state.conversation_id) return false
      try {
        return (
          new URL(url, location.origin).searchParams.get('conversation_id') ===
          state.conversation_id
        )
      } catch (_) {
        return url.includes(`conversation_id=${encodeURIComponent(state.conversation_id)}`)
      }
    })

  if (resultReq) {
    try {
      const resp = await fetch(resultReq, { credentials: 'include' })
      const data = await resp.json()
      if (data?.download_url) {
        const fileId =
          (resultReq.match(/\/backend-api\/files\/download\/(file_[a-zA-Z0-9]+)/) || [])[1] || ''
        return rememberDone(data.download_url, fileId, 'fetchlog_refetch')
      }
    } catch (_) {}
  }

  const generatedImageSelector =
    '[id^="image-"] img[src], img[alt^="已生成图片"][src], img[alt^="Generated image"][src]'
  const generatedImages = [...document.querySelectorAll(generatedImageSelector)]
  const usableGeneratedSrc = (img) => {
    const src = img.getAttribute('src') || ''
    if (!src || src.startsWith('data:image/svg')) return ''
    const width = Number(img.naturalWidth || 0)
    const height = Number(img.naturalHeight || 0)
    // ChatGPT 生成中会先渲染占位图；只有真实图片加载后 natural 尺寸才稳定。
    // 这里必须过滤占位图，否则并发/参考图场景会提前把未完成任务判成 done。
    if (width < 32 || height < 32) return ''
    return src
  }
  const signedImage = generatedImages
    .map((img) => img.getAttribute('src') || '')
    .reverse()
    .find((src) => src.includes('/backend-api/estuary/content') && src.includes('id=file_'))
  if (signedImage) {
    const fileId = (signedImage.match(/[?&]id=(file_[a-zA-Z0-9]+)/) || [])[1] || ''
    if (fileId && !(state.uploaded_refs || []).includes(fileId)) {
      return rememberDone(signedImage, fileId, 'dom_signed_image')
    }
  }

  // 有些结果先渲染 blob/DOM 图片，files/download 可能下一轮才出现。
  const renderedImage = generatedImages.map(usableGeneratedSrc).reverse().find(Boolean)
  if (renderedImage) {
    const fileId = (renderedImage.match(/[?&]id=(file_[a-zA-Z0-9]+)/) || [])[1] || ''
    if (!fileId || !(state.uploaded_refs || []).includes(fileId)) {
      return rememberDone(renderedImage, fileId, 'dom_rendered_image')
    }
  }

  // 如果页面已经渲染出图片卡片，但拦截器还没抓到结果，再等下一轮。
  const imageCards = generatedImages.length
    ? generatedImages
    : document.querySelectorAll('[aria-label="以全视图打开图片"]')

  if (state.conversation_id) {
    await updateStreamStatus(imageCards.length)
    const done = await probeConversationFiles()
    if (done) return done
  }

  if (elapsed > 600000) {
    return JSON.stringify({
      status: 'error',
      error_code: 'generation_timeout',
      message: '生成超时（10min）',
      conversation_id: state.conversation_id || '',
      retryable: true,
      usage_consumed: false,
    })
  }

  await sleep(0)

  return JSON.stringify({
    status: 'generating',
    elapsed: Math.round(elapsed / 1000),
    conversation_id: state.conversation_id || '',
    image_gen_task_id: state.image_gen_task_id || '',
    uploaded_refs: state.uploaded_refs?.length || 0,
    image_cards: imageCards.length,
    fetches: state.fetchLog?.length || 0,
    async_status: state.async_status ?? null,
    stream_status: state.stream_status || '',
  })
})()

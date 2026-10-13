function chatgptApiWaitForImage(auth, conversationId) {
  const generationTimeoutMs = 20 * 60 * 1000
  const started = Date.now()
  let pollDelayMs = CHATGPT_API_FALLBACK_POLL_MS
  let waitingForReadSlot = false
  while (Date.now() - started < generationTimeoutMs) {
    host.call('time.sleep', { milliseconds: pollDelayMs })
    const encoded = encodeURIComponent(conversationId)
    const response = chatgptApiRequest(auth, 'GET', '/backend-api/conversation/' + encoded, {
      target_path: '/backend-api/conversation/' + encoded,
      target_route: '/backend-api/conversation/{conversation_id}',
      timeout_ms: 60000,
    })
    if (chatgptApiMissingConversation(response)) continue
    chatgptApiRequireRetryableRead(response)
    waitingForReadSlot = chatgptApiPollRateLimited(response) && response.coalesced === true
    if (response.ok) {
      const error = chatgptApiConversationError(response.body)
      if (error) throw new Error(error)
      const result = chatgptApiResolveImageCandidate(
        auth,
        conversationId,
        chatgptApiGeneratedImageCandidate(response.body)
      )
      if (result) return result
      pollDelayMs = CHATGPT_API_FALLBACK_POLL_MS
    } else if (chatgptApiPollRateLimited(response)) {
      pollDelayMs = chatgptApiPollRetryDelayMs(response, pollDelayMs)
    } else if (![404, 409, 425].includes(Number(response.status))) {
      chatgptApiRequireOk(response, '轮询 ChatGPT 图片结果')
    }
  }
  if (waitingForReadSlot) throw new Error('temporary_unavailable: 本机查询仍在排队，继续原任务')
  chatgptApiFailConversation('generation_timeout')
}

function chatgptApiGeneratedFileId(conversation) {
  return chatgptApiGeneratedImageCandidate(conversation)?.file_id || ''
}

function chatgptApiGeneratedImageCandidate(conversation) {
  const candidates = []
  // 带参考图的会话里也有文件指针，只能从平台回复或生成产物事件中取结果。
  for (const message of chatgptApiNonUserMessages(conversation)) {
    chatgptApiCollectImageCandidates(message.content, candidates)
  }
  chatgptApiWalk(conversation, (item) => {
    if (!item || typeof item !== 'object') return
    if (item.event === 'artifact' || item.kind === 'generated_image') {
      chatgptApiCollectImageCandidates(item, candidates)
    }
  })
  return candidates[candidates.length - 1] || null
}

function chatgptApiCollectImageCandidates(value, candidates) {
  chatgptApiWalk(value, (item, key) => {
    if (typeof item !== 'string') return
    if (item.startsWith('data:image/')) {
      candidates.push({ data_url: item })
      return
    }
    if (['download_url', 'downloadUrl', 'url'].includes(key) && /^https?:\/\//.test(item)) {
      candidates.push({ url: item })
      return
    }
    if (
      ['asset_pointer', 'assetPointer', 'file_id', 'fileId', 'id'].includes(key) ||
      item.includes('sediment://file') ||
      item.includes('file-service://file')
    ) {
      const match = item.match(/file[-_][a-zA-Z0-9_-]+/)
      if (match) candidates.push({ file_id: match[0] })
    }
  })
}

function chatgptApiResolveImageCandidate(auth, conversationId, candidate) {
  if (!candidate) return null
  if (candidate.file_id) return chatgptApiDownloadResult(auth, conversationId, candidate.file_id)
  const imageData = candidate.data_url || chatgptApiDownloadImageData(auth, candidate.url)
  if (!imageData) return null
  chatgptApiHideConversation(auth, conversationId)
  return {
    url: imageData,
    conversation_id: conversationId,
  }
}

function chatgptApiNonUserMessages(value) {
  const messages = []
  const ids = []
  chatgptApiWalk(value, (item) => {
    if (!item || typeof item !== 'object' || !item.content || !item.author?.role) return
    if (item.author.role === 'user') return
    const identity = String(item.id || '') || JSON.stringify(item.content)
    if (ids.includes(identity)) return
    ids.push(identity)
    messages.push(item)
  })
  return messages
}

function chatgptApiConversationError(conversation) {
  // 只检查平台回复，不能因为用户提示词里出现“rate limit”等字样而误报失败。
  const messages = Object.values(conversation?.mapping || {})
    .map((node) => node?.message)
    .filter((message) => message && message?.author?.role !== 'user')
  const joined = JSON.stringify({
    messages,
    detail: conversation?.detail || '',
    error: conversation?.error || '',
  })
  const cooldownSeconds = chatgptApiCooldownSecondsFromValue(conversation)
  const cooldownSuffix = cooldownSeconds ? '; retry after ' + cooldownSeconds + ' seconds' : ''
  if (/rate_limit_exceeded|generating images too quickly/i.test(joined)) {
    return 'rate_limit_exceeded: ' + joined.slice(0, 300) + cooldownSuffix
  }
  if (/image generations requests|limit resets in/i.test(joined)) {
    return 'quota_exhausted: ' + joined.slice(0, 300) + cooldownSuffix
  }
  return ''
}

function chatgptApiDownloadResult(auth, conversationId, fileId) {
  const path = '/backend-api/files/' + encodeURIComponent(fileId) + '/download'
  const response = chatgptApiRequest(auth, 'GET', path, {
    target_path: path,
    target_route: '/backend-api/files/{file_id}/download',
    timeout_ms: 60000,
  })
  if ([404, 409, 425].includes(Number(response.status))) return null
  const body = chatgptApiRequireOk(response, '获取 ChatGPT 图片下载地址').body
  if (!body?.download_url) return null

  // 签名地址需要浏览器请求头；插件先下载成 data URL，避免后续通用下载器再次被 403 拒绝。
  const imageData = chatgptApiDownloadImageData(auth, body.download_url)
  if (!imageData) return null
  chatgptApiHideConversation(auth, conversationId)
  return {
    url: imageData,
    file_id: fileId,
    conversation_id: conversationId,
  }
}

function chatgptApiDownloadImageData(auth, url) {
  const response = host.call('http.request', {
    method: 'GET',
    url,
    proxy: 'system',
    tls_profile: chatgptApiTlsProfile(auth),
    headers: chatgptApiDownloadImageHeaders(auth, url),
    response_body_format: 'base64',
    timeout_ms: 120000,
  })
  if (chatgptApiImageDownloadPending(response)) return null
  if (!response?.ok || !response?.body_base64) {
    throw new Error('下载 ChatGPT 图片失败：' + chatgptApiHttpSummary(response))
  }
  const contentType = String(response.headers?.['content-type'] || 'image/png').split(';')[0]
  const mime = contentType.startsWith('image/') ? contentType : 'image/png'
  return 'data:' + mime + ';base64,' + response.body_base64
}

function chatgptApiImageDownloadPending(response) {
  return [404, 409, 425].includes(Number(response?.status || 0))
}

function chatgptApiDownloadImageHeaders(auth, url) {
  const sameOrigin = String(url || '').startsWith(CHATGPT_API_ORIGIN + '/')
  const headers = {
    ...chatgptApiBrowserHeaders(auth, sameOrigin ? 'same-origin' : 'cross-site'),
    accept: 'image/avif,image/webp,image/apng,image/*,*/*;q=0.8',
    origin: '',
    referer: CHATGPT_API_ORIGIN + '/',
    'sec-fetch-dest': 'image',
    'sec-fetch-mode': 'no-cors',
    'sec-fetch-site': sameOrigin ? 'same-origin' : 'cross-site',
    authorization: sameOrigin ? 'Bearer ' + auth.auth.access_token : '',
    cookie: sameOrigin ? chatgptApiCookieHeader(auth) : '',
  }
  return chatgptApiCompactObject(headers)
}

function chatgptApiHideConversation(auth, conversationId) {
  try {
    chatgptApiRequest(
      auth,
      'PATCH',
      '/backend-api/conversation/' + encodeURIComponent(conversationId),
      {
        target_path: '/backend-api/conversation/' + encodeURIComponent(conversationId),
        target_route: '/backend-api/conversation/{conversation_id}',
        headers: { 'content-type': 'application/json' },
        body: { is_visible: false },
      }
    )
  } catch (_) {}
}

function chatgptApiFindConversationId(value) {
  let found = ''
  chatgptApiWalk(value, (item, key) => {
    if (!found && key === 'conversation_id' && typeof item === 'string') found = item
  })
  return found
}

function chatgptApiWalk(value, visit, key, depth) {
  const level = Number(depth || 0)
  if (level > 20 || value === null || value === undefined) return
  visit(value, key || '')
  if (Array.isArray(value)) {
    for (const item of value) chatgptApiWalk(item, visit, '', level + 1)
  } else if (typeof value === 'object') {
    for (const [childKey, childValue] of Object.entries(value)) {
      chatgptApiWalk(childValue, visit, childKey, level + 1)
    }
  }
}

function chatgptApiAssertImageAvailable(init) {
  const blocked = Array.isArray(init?.blocked_features) ? init.blocked_features : []
  const imageBlock = blocked.find((item) => item?.name === 'image_gen')
  if (!imageBlock) return
  const message = imageBlock.description || imageBlock.resets_after_text || '图片生成当前不可用'
  if (imageBlock.call_to_action?.includes('authenticate')) {
    throw new Error('LOGIN_EXPIRED: ' + message)
  }
  const seconds =
    chatgptApiCooldownSecondsFromValue(imageBlock) || chatgptApiCooldownSecondsFromText(message)
  throw new Error(
    'quota_exhausted: ' + message + (seconds > 0 ? '; retry after ' + seconds + ' seconds' : '')
  )
}

function chatgptApiUuid() {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (char) => {
    const value = Math.floor(Math.random() * 16)
    return (char === 'x' ? value : (value & 3) | 8).toString(16)
  })
}

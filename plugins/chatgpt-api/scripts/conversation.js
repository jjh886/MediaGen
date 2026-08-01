function chatgptApiGenerateConversation(auth, prompt, aspectRatio, references, generationConfig) {
  const init = chatgptApiInitializeConversation(auth)
  chatgptApiAssertImageAvailable(init)
  const options = chatgptApiConversationOptions(init, generationConfig)
  const prepared = chatgptApiPrepareConversation(auth, references, options)
  const response = chatgptApiSendConversation(auth, prompt, aspectRatio, prepared)
  const conversationId = chatgptApiFindConversationId(response.events || response.body)
  if (!conversationId) throw new Error('ChatGPT 会话响应缺少 conversation_id')
  return {
    model: options.model,
    thinking_effort: options.thinking_effort,
    references: prepared.references,
    result: chatgptApiWaitForImage(auth, conversationId),
  }
}

function chatgptApiConversationOptions(init, generationConfig) {
  return {
    model: String(generationConfig?.model || init?.default_model_slug || 'auto'),
    thinking_effort: chatgptApiThinkingEffort(generationConfig?.thinking_effort),
  }
}

function chatgptApiPrepareConversation(auth, references, options) {
  const turnTraceId = chatgptApiUuid()
  let uploaded = Array.isArray(references) ? references : []
  const needsUpload = uploaded.some((reference) => reference?.base64)
  let conduit = chatgptApiPrepareConduit(
    auth,
    options.model,
    turnTraceId,
    uploaded,
    needsUpload ? 'file_picker' : 'composer_editor_state',
    options.thinking_effort
  )
  if (needsUpload) {
    uploaded = chatgptApiUploadReferences(auth, uploaded)
    // 文件处理完成后，网页会立即用 context_change 重新准备一次会话。
    conduit = chatgptApiPrepareConduit(
      auth,
      options.model,
      turnTraceId,
      uploaded,
      'context_change',
      options.thinking_effort
    )
  }
  return { ...options, references: uploaded, turn_trace_id: turnTraceId, conduit }
}

function chatgptApiSendConversation(auth, prompt, aspectRatio, prepared) {
  const sentinelHeaders = chatgptApiSentinelHeaders(auth)
  const request = chatgptApiConversationPayload(
    auth,
    prepared.model,
    prompt,
    aspectRatio,
    prepared.references,
    prepared.thinking_effort
  )
  return chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/f/conversation', {
      target_route: '/backend-api/f/conversation',
      headers: {
        ...sentinelHeaders,
        accept: 'text/event-stream',
        'content-type': 'application/json',
        'oai-echo-logs': chatgptApiEchoLogs(auth),
        'x-conduit-token': prepared.conduit,
        'x-oai-turn-trace-id': prepared.turn_trace_id,
      },
      body: request,
      sse: true,
      timeout_ms: 120000,
    }),
    '发送 ChatGPT 生图消息'
  )
}

function chatgptApiPrepareConduit(auth, model, turnTraceId, references, source, thinkingEffort) {
  const prepare = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/f/conversation/prepare', {
      target_path: '/backend-api/f/conversation/prepare',
      target_route: '/backend-api/f/conversation/prepare',
      headers: {
        'content-type': 'application/json',
        'x-oai-turn-trace-id': turnTraceId,
      },
      body: chatgptApiConversationPreparePayload(auth, model, references, source, thinkingEffort),
    }),
    '准备 ChatGPT 会话'
  ).body
  if (!prepare?.conduit_token) throw new Error('ChatGPT 会话准备响应缺少 conduit_token')
  return prepare.conduit_token
}

// 会话初始化是独立请求，单独封装后主流程只保留“初始化、准备、发送、轮询”四个步骤。
function chatgptApiInitializeConversation(auth) {
  const fingerprint = chatgptApiFingerprint(auth)
  return chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/conversation/init', {
      target_path: '/backend-api/conversation/init',
      target_route: '/backend-api/conversation/init',
      headers: { 'content-type': 'application/json' },
      body: {
        requested_default_model: null,
        conversation_id: null,
        timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
        conversation_origin: null,
      },
    }),
    '初始化 ChatGPT 会话'
  ).body
}

function chatgptApiConversationPreparePayload(auth, model, references, source, thinkingEffort) {
  const fingerprint = chatgptApiFingerprint(auth)
  const attachments = Array.isArray(references) ? references : []
  const contextChange = source === 'context_change'
  const payload = chatgptApiCompactObject({
    action: 'next',
    parent_message_id: 'client-created-root',
    model,
    client_prepare_state: contextChange ? 'sent' : 'none',
    client_prepare_dispatch: contextChange ? 'immediate' : 'debounced',
    client_prepare_source: source || 'composer_editor_state',
    timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
    timezone: fingerprint.timezone || 'UTC',
    conversation_mode: { kind: 'primary_assistant' },
    system_hints: [],
    attachment_mime_types: chatgptApiUnique(
      attachments.map((reference) => reference.mime_type).filter(Boolean)
    ),
    supports_buffering: true,
    supported_encodings: ['v1'],
    client_contextual_info: {
      app_name: 'chatgpt.com',
      has_web_push_capabilities: true,
      web_push_notification_permission: 'default',
    },
    thinking_effort: chatgptApiThinkingEffort(thinkingEffort),
    local_function_names: ['local.continue_in_work'],
  })
  if (!payload.attachment_mime_types.length) delete payload.attachment_mime_types
  return payload
}

function chatgptApiConversationPayload(
  auth,
  model,
  prompt,
  aspectRatio,
  references,
  thinkingEffort
) {
  const fingerprint = chatgptApiFingerprint(auth)
  const attachments = Array.isArray(references) ? references : []
  const imagePrompt =
    'Generate an image based on this prompt: ' +
    prompt +
    '. Use the aspect ratio ' +
    aspectRatio +
    '.'
  return chatgptApiCompactObject({
    action: 'next',
    messages: [
      {
        id: chatgptApiUuid(),
        author: { role: 'user' },
        create_time: Date.now() / 1000,
        content: chatgptApiMessageContent(attachments, imagePrompt),
        metadata: chatgptApiMessageMetadata(attachments),
      },
    ],
    parent_message_id: 'client-created-root',
    model,
    client_prepare_state: 'success',
    timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
    timezone: fingerprint.timezone || 'UTC',
    conversation_mode: { kind: 'primary_assistant' },
    enable_message_followups: true,
    system_hints: [],
    supports_buffering: true,
    supported_encodings: ['v1'],
    client_contextual_info: {
      is_dark_mode: false,
      time_since_loaded: Math.max(
        0,
        Math.floor((Date.now() - Number(auth.runtime_started_at || Date.now())) / 1000)
      ),
      page_height: Number(fingerprint.page_height || 768),
      page_width: Number(fingerprint.page_width || 1366),
      pixel_ratio: Number(fingerprint.pixel_ratio || 1),
      screen_height: Number(fingerprint.screen_height || 768),
      screen_width: Number(fingerprint.screen_width || 1366),
      app_name: 'chatgpt.com',
      has_web_push_capabilities: true,
      web_push_notification_permission: 'default',
    },
    paragen_cot_summary_display_override: 'allow',
    force_parallel_switch: 'auto',
    thinking_effort: chatgptApiThinkingEffort(thinkingEffort),
    local_function_names: ['local.continue_in_work'],
  })
}

function chatgptApiThinkingEffort(value) {
  // 旧配置没有该字段时保持已经实测通过的 Extended；空字符串表示交给 ChatGPT 决定。
  if (value === undefined || value === null) return 'extended'
  return String(value).trim()
}

function chatgptApiMessageContent(references, prompt) {
  if (!references.length) return { content_type: 'text', parts: [prompt] }
  return {
    content_type: 'multimodal_text',
    parts: [
      ...references.map((reference) => ({
        content_type: 'image_asset_pointer',
        asset_pointer: 'sediment://' + reference.file_id,
        size_bytes: reference.size,
        width: reference.width,
        height: reference.height,
      })),
      prompt,
    ],
  }
}

function chatgptApiAttachmentMetadata(reference) {
  return chatgptApiCompactObject({
    id: reference.file_id,
    size: reference.size,
    name: reference.name,
    mime_type: reference.mime_type,
    width: reference.width,
    height: reference.height,
    source: 'local',
    library_file_id: reference.library_file_id,
    is_big_paste: false,
  })
}

function chatgptApiMessageMetadata(references) {
  const metadata = {
    selected_sources: [],
    serialization_metadata: { custom_symbol_offsets: [] },
  }
  if (references.length) metadata.attachments = references.map(chatgptApiAttachmentMetadata)
  return metadata
}

function chatgptApiEchoLogs(auth) {
  return '0,' + Math.max(0, Date.now() - Number(auth.runtime_started_at || Date.now()))
}

function chatgptApiUnique(values) {
  return values.filter((value, index) => values.indexOf(value) === index)
}

function chatgptApiWaitForImage(auth, conversationId) {
  const generationTimeoutMs = 10 * 60 * 1000
  const started = Date.now()
  while (Date.now() - started < generationTimeoutMs) {
    chatgptApiPollStreamStatus(auth, conversationId)
    const encoded = encodeURIComponent(conversationId)
    const response = chatgptApiRequest(auth, 'GET', '/backend-api/conversation/' + encoded, {
      target_path: '/backend-api/conversation/' + encoded,
      target_route: '/backend-api/conversation/{conversation_id}',
      timeout_ms: 60000,
    })
    if (response.ok) {
      const error = chatgptApiConversationError(response.body)
      if (error) throw new Error(error)
      const fileId = chatgptApiGeneratedFileId(response.body)
      if (fileId) {
        const result = chatgptApiDownloadResult(auth, conversationId, fileId)
        if (result) return result
      }
    } else if (![404, 409, 425].includes(Number(response.status))) {
      chatgptApiRequireOk(response, '轮询 ChatGPT 图片结果')
    }
    host.call('time.sleep', { milliseconds: 2000 })
  }
  throw new Error('generation_timeout: ChatGPT 图片生成超过 10 分钟')
}

function chatgptApiPollStreamStatus(auth, conversationId) {
  const encoded = encodeURIComponent(conversationId)
  const response = chatgptApiRequest(
    auth,
    'GET',
    '/backend-api/conversation/' + encoded + '/stream_status',
    {
      target_path: '/backend-api/conversation/' + encoded + '/stream_status',
      target_route: '/backend-api/conversation/{conversation_id}/stream_status',
      timeout_ms: 60000,
    }
  )
  if (!response.ok && ![404, 409, 425].includes(Number(response.status))) {
    chatgptApiRequireOk(response, '轮询 ChatGPT 会话流状态')
  }
}

function chatgptApiGeneratedFileId(conversation) {
  const candidates = []
  // 带参考图的会话里也有 file_ 指针，只能从平台回复中取结果，绝不能把用户附件返回。
  for (const message of chatgptApiNonUserMessages(conversation)) {
    chatgptApiWalk(message.content, (value, key) => {
      if (typeof value !== 'string') return
      if (key === 'asset_pointer' || value.includes('sediment://file_')) {
        const match = value.match(/file_[a-zA-Z0-9]+/)
        if (match) candidates.push(match[0])
      }
    })
  }
  return candidates[candidates.length - 1] || ''
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
  if (/rate_limit_exceeded|generating images too quickly/i.test(joined)) {
    return 'rate_limit_exceeded: ' + joined.slice(0, 300)
  }
  if (/image generations requests|limit resets in/i.test(joined)) {
    return 'quota_exhausted: ' + joined.slice(0, 300)
  }
  return ''
}

function chatgptApiDownloadResult(auth, conversationId, fileId) {
  const path =
    '/backend-api/files/download/' +
    encodeURIComponent(fileId) +
    '?conversation_id=' +
    encodeURIComponent(conversationId) +
    '&inline=false'
  const response = chatgptApiRequest(auth, 'GET', path, {
    target_path: '/backend-api/files/download/' + encodeURIComponent(fileId),
    target_route: '/backend-api/files/download/{file_id}',
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
  throw new Error('quota_exhausted: ' + message)
}

function chatgptApiUuid() {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (char) => {
    const value = Math.floor(Math.random() * 16)
    return (char === 'x' ? value : (value & 3) | 8).toString(16)
  })
}

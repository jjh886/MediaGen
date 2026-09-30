const CHATGPT_API_SPEECH_VOICES = {
  alloy: 'cove',
  ash: 'fathom',
  coral: 'vale',
  echo: 'ember',
  fable: 'breeze',
  onyx: 'orbit',
  nova: 'maple',
  sage: 'glimmer',
  shimmer: 'juniper',
}

const CHATGPT_API_SPEECH_FORMATS = {
  mp3: 'mp3',
  opus: 'opus',
  aac: 'aac',
}

const CHATGPT_API_SPEECH_MIME_TYPES = {
  mp3: 'audio/mpeg',
  opus: 'audio/ogg',
  aac: 'audio/aac',
}

function chatgptApiGenerateSpeech(auth, input, requestedVoice, requestedFormat) {
  const voice = chatgptApiSpeechVoice(requestedVoice)
  const formats = chatgptApiSpeechFormat(requestedFormat)
  const init = chatgptApiInitializeConversation(auth)
  const sentinelHeaders = chatgptApiSentinelHeaders(auth)
  const prepared = chatgptApiPrepareSpeechConversation(
    auth,
    input,
    String(init?.default_model_slug || 'auto'),
    sentinelHeaders
  )
  const response = chatgptApiSendSpeechMessage(auth, input, prepared, sentinelHeaders)
  const ids = chatgptApiSpeechMessageIds(response.events, input)
  if (!ids.message_id || !ids.conversation_id) {
    throw new Error('temporary_unavailable: ChatGPT TTS 会话响应缺少 message_id 或 conversation_id')
  }
  const audio = chatgptApiDownloadSpeech(
    auth,
    ids.message_id,
    ids.conversation_id,
    voice.platform_voice,
    formats.actual_format
  )
  chatgptApiHideConversation(auth, ids.conversation_id)
  return {
    audio_base64: audio.audio_base64,
    mime_type: audio.mime_type,
    format: formats.actual_format,
    requested_format: formats.requested_format,
    voice: voice.requested_voice,
    platform_voice: voice.platform_voice,
    message_id: ids.message_id,
    conversation_id: ids.conversation_id,
  }
}

function chatgptApiSpeechVoice(value) {
  const requested = String(value || 'alloy')
    .trim()
    .toLowerCase()
  const platform = CHATGPT_API_SPEECH_VOICES[requested]
  if (!platform) throw new Error('unsupported_voice: ChatGPT TTS 不支持音色 ' + requested)
  return { requested_voice: requested, platform_voice: platform }
}

function chatgptApiSpeechFormat(value) {
  const requested = String(value || 'mp3')
    .trim()
    .toLowerCase()
  const actual = CHATGPT_API_SPEECH_FORMATS[requested]
  if (!actual) throw new Error('unsupported_format: ChatGPT TTS 不支持格式 ' + requested)
  return { requested_format: requested, actual_format: actual }
}

function chatgptApiSpeechPrompt(input) {
  return '请逐字复述以下文本，只输出原文，不添加引号、解释或其他内容：\n\n' + input
}

function chatgptApiPrepareSpeechConversation(auth, input, model, sentinelHeaders) {
  const prepared = chatgptApiPrepareTextConversation(
    auth,
    chatgptApiSpeechPrompt(input),
    { parent_message_id: 'client-created-root' },
    { model },
    sentinelHeaders
  )
  return { ...prepared, message_id: prepared.user_message_id }
}

function chatgptApiSendSpeechMessage(auth, input, prepared, sentinelHeaders) {
  const events = []
  const result = chatgptApiStreamTextConversation(
    auth,
    chatgptApiSpeechPrompt(input),
    prepared,
    sentinelHeaders,
    [],
    (event) => events.push(event)
  )
  chatgptApiRequireOk(result, '发送 ChatGPT TTS 消息')
  return { ...result, events }
}

function chatgptApiSpeechMessageIds(events, input) {
  const state = { conversation_id: '', message: {}, exact_message_id: '' }
  for (const event of Array.isArray(events) ? events : []) {
    chatgptApiApplySpeechEvent(state, event, input)
  }
  return {
    conversation_id: state.conversation_id,
    message_id: state.exact_message_id,
  }
}

function chatgptApiApplySpeechEvent(state, event, input) {
  if (!event || typeof event !== 'object') return
  state.conversation_id =
    state.conversation_id || String(event.conversation_id || event.v?.conversation_id || '')
  if (Array.isArray(event.v)) {
    for (const operation of event.v) chatgptApiApplySpeechPatch(state, operation, input)
    return
  }
  if (typeof event.p === 'string') {
    chatgptApiApplySpeechPatch(state, event, input)
    return
  }
  const value = event.v && typeof event.v === 'object' ? event.v : event
  if (value.message) chatgptApiRememberSpeechMessage(state, value.message, input)
}

function chatgptApiApplySpeechPatch(state, operation, input) {
  const path = String(operation?.p || '')
  const value = operation?.v
  if (path === '/conversation_id') state.conversation_id = String(value || '')
  else if (path === '/message') state.message = value || {}
  else if (path === '/message/id') state.message.id = String(value || '')
  else if (path === '/message/author/role') {
    state.message.author = { ...(state.message.author || {}), role: String(value || '') }
  } else if (path === '/message/content/parts') {
    state.message.content = { ...(state.message.content || {}), parts: value }
  } else if (path.startsWith('/message/content/parts/0') && typeof value === 'string') {
    const current = String(state.message.content?.parts?.[0] || '')
    state.message.content = {
      ...(state.message.content || {}),
      parts: [operation.o === 'append' ? current + value : value],
    }
  }
  chatgptApiRememberSpeechMessage(state, state.message, input)
}

function chatgptApiRememberSpeechMessage(state, message, input) {
  if (message?.author?.role !== 'assistant' || !message?.id) return
  const text = (Array.isArray(message.content?.parts) ? message.content.parts : [])
    .filter((part) => typeof part === 'string')
    .join('')
  if (text === input && text.length > 0) {
    state.exact_message_id = String(message.id)
  } else if (state.exact_message_id === String(message.id)) {
    state.exact_message_id = ''
  }
}

function chatgptApiDownloadSpeech(auth, messageId, conversationId, voice, format) {
  const path =
    '/backend-api/synthesize?message_id=' +
    encodeURIComponent(messageId) +
    '&conversation_id=' +
    encodeURIComponent(conversationId) +
    '&voice=' +
    encodeURIComponent(voice) +
    '&format=' +
    encodeURIComponent(format)
  const response = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'GET', path, {
      target_path: '/backend-api/synthesize',
      target_route: '/backend-api/synthesize',
      headers: { accept: 'audio/*,*/*' },
      response_body_format: 'base64',
      timeout_ms: 20 * 60 * 1000,
    }),
    '下载 ChatGPT TTS 音频'
  )
  return chatgptApiResolveSpeechResponse(auth, response, format)
}

function chatgptApiResolveSpeechResponse(auth, response, format) {
  const contentType = String(response.headers?.['content-type'] || '')
    .split(';')[0]
    .toLowerCase()
  const encoded = String(response.body_base64 || '')
  if (!encoded) throw new Error('temporary_unavailable: ChatGPT TTS 返回了空音频')
  if (contentType.includes('json')) {
    const decoded = chatgptApiBase64DecodeBinary(encoded)
    let body
    try {
      body = JSON.parse(decoded)
    } catch (_) {
      throw new Error('temporary_unavailable: ChatGPT TTS 下载地址响应不是有效 JSON')
    }
    const url = String(body.download_url || body.url || '')
    if (!url) throw new Error('temporary_unavailable: ChatGPT TTS 响应缺少下载地址')
    return chatgptApiDownloadSpeechUrl(auth, url, format)
  }
  const mime = contentType.startsWith('audio/')
    ? contentType
    : CHATGPT_API_SPEECH_MIME_TYPES[format]
  return { audio_base64: encoded, mime_type: mime }
}

function chatgptApiDownloadSpeechUrl(auth, url, format) {
  const response = host.call('http.request', {
    method: 'GET',
    url,
    proxy: 'system',
    tls_profile: chatgptApiTlsProfile(auth),
    headers: {
      ...chatgptApiDownloadImageHeaders(auth, url),
      accept: 'audio/*,*/*',
      'sec-fetch-dest': 'audio',
    },
    response_body_format: 'base64',
    timeout_ms: 180000,
  })
  if (!response?.ok || !response?.body_base64) {
    throw new Error(
      'temporary_unavailable: 下载 ChatGPT TTS 签名音频失败：' + chatgptApiHttpSummary(response)
    )
  }
  const contentType = String(response.headers?.['content-type'] || '')
    .split(';')[0]
    .toLowerCase()
  return {
    audio_base64: response.body_base64,
    mime_type: contentType.startsWith('audio/')
      ? contentType
      : CHATGPT_API_SPEECH_MIME_TYPES[format],
  }
}

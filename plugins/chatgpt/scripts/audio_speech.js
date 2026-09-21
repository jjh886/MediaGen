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
  chatgptApiInitializeConversation(auth)
  const sentinelHeaders = chatgptApiSentinelHeaders(auth)
  const prepared = chatgptApiPrepareSpeechConversation(auth, input, 'auto', sentinelHeaders)
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

function chatgptApiPrepareSpeechConversation(auth, input, model, sentinelHeaders) {
  const turnTraceId = chatgptApiUuid()
  let conduit = ''
  for (const state of ['none', 'sent', 'success']) {
    const response = chatgptApiRequireOk(
      chatgptApiRequest(auth, 'POST', '/backend-api/f/conversation/prepare', {
        target_path: '/backend-api/f/conversation/prepare',
        target_route: '/backend-api/f/conversation/prepare',
        headers: chatgptApiCompactObject({
          ...sentinelHeaders,
          'content-type': 'application/json',
          'x-conduit-token': conduit,
          'x-oai-turn-trace-id': turnTraceId,
        }),
        body: chatgptApiSpeechPreparePayload(auth, input, model, state),
        retry: { max_attempts: 1 },
        timeout_ms: 60000,
      }),
      '准备 ChatGPT TTS 会话（' + state + '）'
    ).body
    if (!response || (response.status && response.status !== 'ok')) {
      throw new Error('temporary_unavailable: ChatGPT TTS 会话 prepare 响应无效')
    }
    conduit = typeof response.conduit_token === 'string' ? response.conduit_token : ''
  }
  return {
    model,
    turn_trace_id: turnTraceId,
    conduit,
    parent_message_id: 'client-created-root',
    message_id: chatgptApiUuid(),
  }
}

function chatgptApiSpeechPreparePayload(auth, input, model, state) {
  const fingerprint = chatgptApiFingerprint(auth)
  const payload = {
    action: 'next',
    parent_message_id: 'client-created-root',
    model,
    client_prepare_state: state,
    client_prepare_dispatch: 'debounced',
    client_prepare_source: 'composer_editor_state',
    timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
    timezone: fingerprint.timezone || 'UTC',
    conversation_mode: { kind: 'primary_assistant' },
    system_hints: [],
    supports_buffering: true,
    supported_encodings: ['v1'],
    client_contextual_info: chatgptApiSpeechClientContext(auth),
    local_function_names: ['local.continue_in_work'],
  }
  if (state !== 'none') {
    payload.partial_query = {
      id: chatgptApiUuid(),
      author: { role: 'user' },
      content: { content_type: 'text', parts: [Array.from(input).slice(0, 5).join('') || 'h'] },
    }
  }
  return payload
}

function chatgptApiSpeechClientContext(auth) {
  const fingerprint = chatgptApiFingerprint(auth)
  return {
    app_name: 'chatgpt.com',
    is_dark_mode: false,
    time_since_loaded: Math.max(
      0,
      Math.round((Date.now() - Number(auth.runtime_started_at || Date.now())) / 1000)
    ),
    page_height: Number(fingerprint.page_height || 768),
    page_width: Number(fingerprint.page_width || 1366),
    pixel_ratio: Number(fingerprint.pixel_ratio || 1),
    screen_height: Number(fingerprint.screen_height || 768),
    screen_width: Number(fingerprint.screen_width || 1366),
  }
}

function chatgptApiSendSpeechMessage(auth, input, prepared, sentinelHeaders) {
  const fingerprint = chatgptApiFingerprint(auth)
  return chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/f/conversation', {
      target_path: '/backend-api/f/conversation',
      target_route: '/backend-api/f/conversation',
      headers: chatgptApiCompactObject({
        ...sentinelHeaders,
        accept: 'text/event-stream',
        'content-type': 'application/json',
        'oai-echo-logs': chatgptApiEchoLogs(auth),
        'x-conduit-token': prepared.conduit,
        'x-oai-turn-trace-id': prepared.turn_trace_id,
      }),
      body: {
        action: 'next',
        messages: [
          {
            id: prepared.message_id,
            author: { role: 'assistant' },
            create_time: Date.now() / 1000,
            content: { content_type: 'text', parts: [input] },
            metadata: chatgptApiSpeechMessageMetadata(),
          },
        ],
        parent_message_id: prepared.parent_message_id,
        model: prepared.model,
        client_prepare_state: 'success',
        timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
        timezone: fingerprint.timezone || 'UTC',
        conversation_mode: { kind: 'primary_assistant' },
        history_and_training_disabled: false,
        system_hints: [],
        paragen_cot_summary_display_override: 'allow',
        force_parallel_switch: 'auto',
        thinking_effort: 'standard',
      },
      sse: true,
      retry: { max_attempts: 1 },
      timeout_ms: 3 * 60 * 60 * 1000,
    }),
    '发送 ChatGPT TTS 消息'
  )
}

function chatgptApiSpeechMessageMetadata() {
  return {
    developer_mode_connector_ids: [],
    selected_sources: [],
    selected_github_repos: [],
    selected_all_github_repos: false,
    serialization_metadata: { custom_symbol_offsets: [] },
  }
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
  if (typeof event.p === 'string') {
    if (event.p === '' && event.o === 'patch' && Array.isArray(event.v)) {
      for (const operation of event.v) chatgptApiApplySpeechPatch(state, operation, input)
    } else {
      chatgptApiApplySpeechPatch(state, event, input)
    }
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

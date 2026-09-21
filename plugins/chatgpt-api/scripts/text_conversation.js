const CHATGPT_API_TEXT_POLL_MS = 2000
const CHATGPT_API_TEXT_TIMEOUT_MS = 20 * 60 * 1000

function chatgptApiTextOptions(init, request) {
  const requestedModel = String(request.model || '').trim()
  if (requestedModel.endsWith('-wm')) {
    throw new Error('invalid_input: Work 模型需要独立的 Work 任务，不能用于普通文本生成')
  }
  const aliases = ['chatgpt api text', 'chatgpt-api-text', 'chatgpt-web']
  const model = aliases.includes(requestedModel.toLowerCase())
    ? String(init?.default_model_slug || 'auto')
    : requestedModel || String(init?.default_model_slug || 'auto')
  return {
    model,
    system_hints: [],
    thinking_effort: request.reasoning.effort,
  }
}

function chatgptApiPrepareTextConversation(auth, prompt, thread, options, sentinelHeaders) {
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
        body: chatgptApiTextPreparePayload(auth, prompt, thread, options, state),
        retry: { max_attempts: 1 },
        timeout_ms: 60000,
      }),
      '准备 ChatGPT 文本会话（' + state + '）'
    ).body
    if (!response || (response.status && response.status !== 'ok')) {
      throw new Error('temporary_unavailable: ChatGPT 文本会话 prepare 响应无效')
    }
    conduit = typeof response.conduit_token === 'string' ? response.conduit_token : ''
  }
  return {
    ...thread,
    ...options,
    turn_trace_id: turnTraceId,
    conduit,
    user_message_id: chatgptApiUuid(),
  }
}

function chatgptApiTextPreparePayload(auth, prompt, thread, options, state) {
  const fingerprint = chatgptApiFingerprint(auth)
  const payload = chatgptApiCompactObject({
    action: 'next',
    parent_message_id: thread.parent_message_id,
    conversation_id: thread.conversation_id,
    model: options.model,
    client_prepare_state: state,
    client_prepare_dispatch: 'debounced',
    client_prepare_source: 'composer_editor_state',
    timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
    timezone: fingerprint.timezone || 'UTC',
    conversation_mode: { kind: 'primary_assistant' },
    system_hints: options.system_hints || [],
    supports_buffering: true,
    supported_encodings: ['v1'],
    client_contextual_info: chatgptApiTextClientContext(auth),
    attachment_mime_types: (options.references || []).map((reference) => reference.mime_type),
    local_function_names: ['local.continue_in_work'],
  })
  if (state !== 'none') {
    payload.partial_query = {
      id: chatgptApiUuid(),
      author: { role: 'user' },
      content: { content_type: 'text', parts: [Array.from(prompt).slice(0, 5).join('') || 'h'] },
    }
  }
  return payload
}

function chatgptApiTextClientContext(auth) {
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

function chatgptApiStreamTextConversation(auth, prompt, prepared, sentinelHeaders, onEvent) {
  const path = '/backend-api/f/conversation'
  const headers = chatgptApiBaseHeaders(auth, path, path)
  Object.assign(
    headers,
    chatgptApiCompactObject({
      ...sentinelHeaders,
      accept: 'text/event-stream',
      'content-type': 'application/json',
      'oai-echo-logs': chatgptApiEchoLogs(auth),
      'x-conduit-token': prepared.conduit,
      'x-oai-turn-trace-id': prepared.turn_trace_id,
    })
  )
  return chatgptApiStreamWithResume(
    auth,
    {
      method: 'POST',
      url: CHATGPT_API_ORIGIN + path,
      proxy: 'system',
      headers,
      body: chatgptApiTextConversationPayload(auth, prompt, prepared),
      retry: { max_attempts: 1 },
      timeout_ms: 3 * 60 * 60 * 1000,
    },
    (event) => {
      onEvent(event)
      return null
    }
  )
}

function chatgptApiCreateTextStreamState() {
  return {
    conversation_id: '',
    message_id: '',
    role: '',
    channel: '',
    text: '',
    final_text: '',
    finished: false,
    streamed_message_id: '',
    streamed_text: '',
  }
}

function chatgptApiConsumeTextStreamEvent(state, writer, event, onConversation) {
  const previousMessageId = state.message_id
  chatgptApiApplyTextEvent(state, event)
  if (state.conversation_id && onConversation) onConversation(state.conversation_id)
  if (state.message_id && state.message_id !== previousMessageId) {
    state.streamed_message_id = state.message_id
    state.streamed_text = ''
  }
  if (!chatgptApiIsFinalText(state) || (!state.channel && !state.finished)) return
  const current = String(state.text || '')
  if (!current) return
  // 快照替换和消息切换不能当作增量追加；完成时以最终快照校准。
  const emitted = state.emitted_text || ''
  if (!current.startsWith(emitted)) return
  state.streamed_text = current
  state.emitted_text = current
  chatgptApiFeedResponseText(writer, current.slice(emitted.length))
}

function chatgptApiAppendMissingFinalText(state, writer, value) {
  const finalText = String(value || '')
  if (!finalText) return
  const emitted = state.emitted_text || ''
  if (finalText.startsWith(emitted)) {
    chatgptApiFeedResponseText(writer, finalText.slice(emitted.length))
  } else {
    // response.completed / output_text.done 返回权威文本，工具调用也按最终正文重新解析。
    writer.output = []
    writer.message = null
    writer.text = ''
    writer.parser = { buffer: '', inside: false }
    writer.silent = true
    chatgptApiFeedResponseText(writer, finalText)
    chatgptApiDrainToolParser(writer, true)
    writer.silent = false
  }
  state.emitted_text = finalText
}

function chatgptApiTextConversationPayload(auth, prompt, prepared) {
  const fingerprint = chatgptApiFingerprint(auth)
  return chatgptApiCompactObject({
    action: 'next',
    messages: [
      {
        id: prepared.user_message_id,
        author: { role: 'user' },
        create_time: Date.now() / 1000,
        content: chatgptApiMessageContent(prepared.references || [], prompt),
        metadata: {
          ...chatgptApiTextMessageMetadata(),
          attachments: (prepared.references || []).map(chatgptApiAttachmentMetadata),
        },
      },
    ],
    parent_message_id: prepared.parent_message_id,
    conversation_id: prepared.conversation_id,
    model: prepared.model,
    client_prepare_state: 'success',
    timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
    timezone: fingerprint.timezone || 'UTC',
    conversation_mode: { kind: 'primary_assistant' },
    history_and_training_disabled: true,
    system_hints: prepared.system_hints || [],
    paragen_cot_summary_display_override: 'allow',
    force_parallel_switch: 'auto',
    thinking_effort: prepared.thinking_effort,
  })
}

function chatgptApiTextMessageMetadata() {
  return {
    developer_mode_connector_ids: [],
    selected_sources: [],
    selected_github_repos: [],
    selected_all_github_repos: false,
    serialization_metadata: { custom_symbol_offsets: [] },
  }
}

function chatgptApiParseTextEvents(events) {
  const state = {
    conversation_id: '',
    message_id: '',
    role: '',
    channel: '',
    text: '',
    final_text: '',
    finished: false,
  }
  for (const event of Array.isArray(events) ? events : []) chatgptApiApplyTextEvent(state, event)
  return {
    conversation_id: state.conversation_id,
    output_text: String(state.finished && chatgptApiIsFinalText(state) ? state.text : '').trim(),
  }
}

function chatgptApiApplyTextEvent(state, event) {
  if (!event || typeof event !== 'object') return
  state.conversation_id =
    state.conversation_id || String(event.conversation_id || event.v?.conversation_id || '')
  if (Array.isArray(event.choices)) {
    chatgptApiApplyTextChunk(state, event)
    return
  }
  // 网页 2026-08 起还会发送不带 p 的补丁数组。
  if (Array.isArray(event.v)) {
    for (const operation of event.v) chatgptApiApplyTextPatch(state, operation)
    return
  }
  if (typeof event.p === 'string') {
    if (event.p === '' && event.o === 'patch' && Array.isArray(event.v)) {
      for (const operation of event.v) chatgptApiApplyTextPatch(state, operation)
    } else {
      chatgptApiApplyTextPatch(state, event)
    }
    return
  }
  if (typeof event.v === 'string') {
    state.text = chatgptApiAppendText(state.text, event.v)
    chatgptApiRememberFinalText(state)
    return
  }
  const value = event.v && typeof event.v === 'object' ? event.v : event
  if (value.message) chatgptApiApplyFullTextMessage(state, value)
}

function chatgptApiApplyTextChunk(state, event) {
  const choice = event.choices[0] || {}
  const delta = choice.delta || {}
  const channel = String(delta.channel || choice.channel || event.channel || '')
  if (channel) chatgptApiSetTextChannel(state, channel)
  else if (!state.channel) state.channel = 'final'
  if (delta.role) state.role = String(delta.role)
  if (typeof delta.content === 'string') {
    state.text = chatgptApiAppendText(state.text, delta.content)
    chatgptApiRememberFinalText(state)
  }
  if (choice.finish_reason) state.finished = true
}

function chatgptApiApplyTextPatch(state, operation) {
  if (!operation || typeof operation !== 'object') return
  const path = String(operation.p || '')
  const value = operation.v
  if (path === '/conversation_id') state.conversation_id = String(value || '')
  else if (path === '/message') chatgptApiApplyFullTextMessage(state, { message: value })
  else if (path === '/message/id') {
    const nextId = String(value || '')
    if (state.message_id && state.message_id !== nextId) {
      state.text = ''
      state.final_text = ''
      state.role = ''
      state.channel = ''
      state.finished = false
    }
    state.message_id = nextId
  } else if (path === '/message/channel') chatgptApiSetTextChannel(state, String(value || ''))
  else if (path === '/message/author/role') state.role = String(value || '')
  else if (path === '/message/content/parts' && Array.isArray(value)) {
    state.text = chatgptApiTextParts(value)
  } else if (path.startsWith('/message/content/parts/0') && typeof value === 'string') {
    state.text = operation.o === 'append' ? state.text + value : value
  } else if (path === '/message/end_turn' && value === true) state.finished = true
  else if (path === '/message/status' && value === 'finished_successfully') state.finished = true
  else if (path.startsWith('/message/metadata/finish_details') && value) state.finished = true
  chatgptApiRememberFinalText(state)
}

function chatgptApiApplyFullTextMessage(state, value) {
  const message = value?.message
  if (!message || message.author?.role === 'user') return
  const messageId = String(message.id || '')
  if (state.message_id && messageId && state.message_id !== messageId) {
    state.text = ''
    state.final_text = ''
    state.role = ''
    state.channel = ''
    state.finished = false
  }
  state.message_id = messageId || state.message_id
  state.role = String(message.author?.role || state.role)
  state.channel = String(message.channel || value.channel || state.channel)
  state.text = chatgptApiTextParts(message.content?.parts)
  state.finished = chatgptApiTextMessageFinished(message) || state.finished
  chatgptApiRememberFinalText(state)
}

function chatgptApiTextMessageFinished(message) {
  return Boolean(
    message?.end_turn === true ||
    message?.status === 'finished_successfully' ||
    message?.metadata?.finish_details
  )
}

function chatgptApiSetTextChannel(state, channel) {
  if (state.channel && state.channel !== channel) {
    state.text = ''
    state.final_text = ''
    state.finished = false
  }
  state.channel = channel
}

function chatgptApiIsFinalText(state) {
  return (
    (!state.role || state.role === 'assistant') && (!state.channel || state.channel === 'final')
  )
}

function chatgptApiRememberFinalText(state) {
  if (!chatgptApiIsFinalText(state)) return
  if (state.text.trim()) state.final_text = state.text
}

function chatgptApiAppendText(current, incoming) {
  if (!incoming) return current
  // 此函数只接收 delta；重复引号、括号、字词都是合法内容。
  return current + incoming
}

function chatgptApiTextParts(parts) {
  return (Array.isArray(parts) ? parts : []).filter((part) => typeof part === 'string').join('')
}

function chatgptApiWaitForText(auth, conversationId, baselineParentId) {
  const started = Date.now()
  while (Date.now() - started < CHATGPT_API_TEXT_TIMEOUT_MS) {
    const conversation = chatgptApiFetchTextConversation(auth, conversationId)
    const error = chatgptApiTextConversationError(conversation)
    if (error) throw new Error(error)
    const result = chatgptApiFinalTextFromConversation(conversation, baselineParentId)
    if (result) return result
    host.call('time.sleep', { milliseconds: CHATGPT_API_TEXT_POLL_MS })
  }
  throw new Error('generation_timeout: ChatGPT 文本生成超过 20 分钟')
}

function chatgptApiTextConversationError(conversation) {
  const detail = conversation?.detail || conversation?.error
  if (detail) {
    const message = typeof detail === 'string' ? detail : detail.message || JSON.stringify(detail)
    return 'ChatGPT 文本生成失败：' + String(message).slice(0, 400)
  }
  return ''
}

function chatgptApiFetchTextConversation(auth, conversationId) {
  const path = '/backend-api/conversation/' + encodeURIComponent(conversationId)
  const response = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'GET', path, {
      target_path: path,
      target_route: '/backend-api/conversation/{conversation_id}',
      timeout_ms: 60000,
    }),
    '轮询 ChatGPT 文本结果'
  )
  return response.body || {}
}

function chatgptApiFinalTextFromConversation(conversation, baselineParentId) {
  const mapping = conversation?.mapping
  let nodeId = String(conversation?.current_node || '')
  if (!mapping || !nodeId || nodeId === baselineParentId) return ''
  const candidates = []
  let reachedBaseline = baselineParentId === 'client-created-root'
  const visited = []
  while (nodeId && mapping[nodeId] && !visited.includes(nodeId)) {
    visited.push(nodeId)
    if (nodeId === baselineParentId) {
      reachedBaseline = true
      break
    }
    const message = mapping[nodeId].message
    const channel = String(message?.channel || '')
    const text = chatgptApiTextParts(message?.content?.parts).trim()
    const finished =
      message?.end_turn === true ||
      Boolean(message?.metadata?.finish_details) ||
      message?.status === 'finished_successfully'
    if (
      message?.author?.role === 'assistant' &&
      (!channel || channel === 'final') &&
      text &&
      finished
    ) {
      candidates.push(text)
    }
    nodeId = String(mapping[nodeId].parent || '')
  }
  if (!reachedBaseline && baselineParentId !== 'client-created-root') return ''
  return candidates[0] || ''
}

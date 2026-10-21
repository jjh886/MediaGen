const CHATGPT_API_TEXT_POLL_MS = CHATGPT_API_FALLBACK_POLL_MS
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

function chatgptApiPrepareTextConversation(
  auth,
  prompt,
  thread,
  options,
  _sentinelHeaders,
  attachments = []
) {
  const turnTraceId = apiTextUuid()
  const response = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/f/conversation/prepare', {
      target_path: '/backend-api/f/conversation/prepare',
      target_route: '/backend-api/f/conversation/prepare',
      headers: chatgptApiCompactObject({
        'content-type': 'application/json',
        'x-oai-turn-trace-id': turnTraceId,
      }),
      body: chatgptApiTextPreparePayload(auth, prompt, thread, options, 'sent', attachments),
      retry: { max_attempts: 1 },
      timeout_ms: 60000,
    }),
    '准备 ChatGPT 文本会话'
  ).body
  if (!response || (response.status && response.status !== 'ok')) {
    throw new Error('temporary_unavailable: ChatGPT 文本会话 prepare 响应无效')
  }
  chatgptApiRequireOk(
    chatgptApiRequest(auth, 'GET', '/backend-api/system_hints', {
      target_path: '/backend-api/system_hints',
      target_route: '/backend-api/system_hints',
      timeout_ms: 30000,
    }),
    '读取 ChatGPT 会话能力'
  )
  const conduit = typeof response.conduit_token === 'string' ? response.conduit_token : ''
  return {
    ...thread,
    ...options,
    turn_trace_id: turnTraceId,
    conduit,
    user_message_id: apiTextUuid(),
  }
}

function chatgptApiTextPreparePayload(auth, prompt, thread, options, state, attachments = []) {
  const fingerprint = chatgptApiFingerprint(auth)
  const payload = chatgptApiCompactObject({
    action: 'next',
    is_do_not_remember: false,
    parent_message_id: thread.parent_message_id,
    conversation_id: thread.conversation_id,
    model: options.model,
    client_prepare_state: state,
    client_prepare_dispatch: 'debounced',
    client_prepare_source: 'composer_editor_state',
    timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
    timezone: fingerprint.timezone || 'UTC',
    local_function_names: ['local.continue_in_work'],
    client_contextual_info: chatgptApiTextClientContext(auth),
    supported_encodings: ['v1'],
  })
  if (attachments.length) {
    payload.attachment_mime_types = [...new Set(attachments.map((item) => item.mime_type))]
  }
  if (state !== 'none') {
    payload.partial_query = {
      id: apiTextUuid(),
      author: { role: 'user' },
      content: { content_type: 'text', parts: [prompt] },
    }
  }
  return payload
}

function chatgptApiStreamTextConversation(
  auth,
  prompt,
  prepared,
  sentinelHeaders,
  attachments,
  onEvent
) {
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
      body: chatgptApiTextConversationPayload(auth, prompt, prepared, attachments),
      stop_condition: "type == 'message_stream_complete'",
      retry: { max_attempts: 1 },
      timeout_ms: chatgptApiFixedVideoAnalysis ? CHATGPT_API_TEXT_TIMEOUT_MS : 3 * 60 * 60 * 1000,
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
  chatgptApiConsumeRichTextEvent(state, writer, event)
  if (state.rich_message?.metadata?.is_visually_hidden_from_conversation) return
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
  apiTextFeedResponseText(writer, current.slice(emitted.length))
}

function chatgptApiAppendMissingFinalText(state, writer, value) {
  const finalText = String(value || '')
  if (!finalText) return
  const emitted = state.emitted_text || ''
  if (finalText.startsWith(emitted)) {
    apiTextFeedResponseText(writer, finalText.slice(emitted.length))
  } else {
    // response.completed / output_text.done 返回权威文本，工具调用也按最终正文重新解析。
    writer.output = []
    writer.message = null
    writer.text = ''
    writer.parser = { buffer: '', inside: false }
    writer.silent = true
    apiTextFeedResponseText(writer, finalText)
    apiTextDrainToolParser(writer, true)
    writer.silent = false
  }
  state.emitted_text = finalText
}

function chatgptApiTextConversationPayload(auth, prompt, prepared, attachments = []) {
  const fingerprint = chatgptApiFingerprint(auth)
  const metadata = chatgptApiTextMessageMetadata()
  if (attachments.length) {
    // 当前上传器走网页保留的 files/process_upload_stream 链路。
    metadata.file_upload_slot_prefetch_attribution = {
      actual_path: 'legacy',
      reason: 'direct_library_multipart_preserved',
    }
    metadata.attachments = attachments.map((attachment) => ({
      id: attachment.file_id,
      library_file_id: attachment.library_file_id || '',
      mime_type: attachment.mime_type,
      name: attachment.name,
      size: attachment.size,
    }))
  }
  return chatgptApiCompactObject({
    turn_attribution: { turn_trigger: 'composer' },
    action: 'next',
    is_do_not_remember: false,
    messages: [
      {
        id: prepared.user_message_id,
        author: { metadata: {}, name: null, role: 'user' },
        channel: null,
        create_time: Date.now() / 1000,
        content: chatgptApiMessageContent(
          attachments.filter((item) => item.mime_type.startsWith('image/')),
          prompt
        ),
        metadata,
        end_turn: null,
        recipient: 'all',
        status: 'finished_successfully',
        update_time: null,
        weight: 1,
      },
    ],
    parent_message_id: prepared.parent_message_id,
    conversation_id: prepared.conversation_id,
    model: prepared.model,
    client_prepare_state: 'success',
    genui_state_snapshots: [],
    timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
    timezone: fingerprint.timezone || 'UTC',
    supported_encodings: ['v1'],
    client_contextual_info: chatgptApiTextClientContext(auth),
    local_function_names: ['local.continue_in_work'],
    thinking_effort: prepared.thinking_effort,
  })
}

function chatgptApiTextMessageMetadata() {
  return {}
}

function chatgptApiWaitForText(auth, conversationId, baselineParentId) {
  const started = Date.now()
  const checkpoint = host.checkpoint() || {}
  const sameConversation = checkpoint.conversation_id === conversationId
  let delay = sameConversation
    ? chatgptApiClampPollDelay(checkpoint.poll_retry_ms || CHATGPT_API_TEXT_POLL_MS, 3600000)
    : CHATGPT_API_TEXT_POLL_MS
  let nextPollAt = sameConversation ? Number(checkpoint.poll_next_at_ms || 0) : 0
  let waitingForReadSlot = false
  while (Date.now() - started < CHATGPT_API_TEXT_TIMEOUT_MS) {
    if (nextPollAt > Date.now()) {
      host.call('time.sleep', {
        milliseconds: Math.min(
          nextPollAt - Date.now(),
          CHATGPT_API_TEXT_TIMEOUT_MS - (Date.now() - started)
        ),
      })
      if (Date.now() - started >= CHATGPT_API_TEXT_TIMEOUT_MS) break
    }
    const response = chatgptApiTextConversationResponse(auth, conversationId)
    chatgptApiRequireRetryableRead(response, conversationId)
    waitingForReadSlot = chatgptApiPollRateLimited(response) && response.coalesced === true
    if (chatgptApiPollRateLimited(response)) {
      delay = chatgptApiPollRetryDelayMs(response, delay)
      nextPollAt = Date.now() + delay
      if (sameConversation) {
        host.checkpoint({
          ...host.checkpoint(),
          poll_retry_ms: delay,
          poll_next_at_ms: nextPollAt,
          poll_rate_limited: true,
          poll_rate_limit_source: 'local',
        })
      }
      continue
    }
    const conversation = chatgptApiRequireOk(response, '轮询 ChatGPT 文本结果').body || {}
    delay = CHATGPT_API_TEXT_POLL_MS
    nextPollAt = 0
    if (sameConversation) {
      host.checkpoint({ ...host.checkpoint(), poll_retry_ms: delay, poll_next_at_ms: 0 })
    }
    const error = chatgptApiTextConversationError(conversation)
    if (error) chatgptApiFailConversation('upstream_failed')
    const result = chatgptApiFinalTextFromConversation(conversation, baselineParentId)
    if (result) return result
    host.call('time.sleep', { milliseconds: delay })
  }
  if (waitingForReadSlot) throw new Error('temporary_unavailable: 本机查询仍在排队，继续原任务')
  chatgptApiFailConversation('generation_timeout')
}

function chatgptApiTextConversationError(conversation) {
  const status = String(conversation?.async_status || conversation?.status || '').toLowerCase()
  if (['failed', 'error', 'cancelled', 'canceled'].includes(status)) return 'ChatGPT 原任务已失败'
  const detail = conversation?.detail || conversation?.error
  if (detail) {
    const message = typeof detail === 'string' ? detail : detail.message || JSON.stringify(detail)
    return 'ChatGPT 文本生成失败：' + String(message).slice(0, 400)
  }
  return ''
}

function chatgptApiFetchTextConversation(auth, conversationId, timeoutMs = 60000) {
  const response = chatgptApiRequireOk(
    chatgptApiTextConversationResponse(auth, conversationId, timeoutMs),
    '轮询 ChatGPT 文本结果'
  )
  return response.body || {}
}

function chatgptApiTextConversationResponse(auth, conversationId, timeoutMs = 60000) {
  const path = '/backend-api/conversations/' + encodeURIComponent(conversationId)
  return chatgptApiRequest(auth, 'GET', path, {
    target_path: path,
    target_route: '/backend-api/conversations/{conversation_id}',
    timeout_ms: timeoutMs,
    retry: { max_attempts: 1 },
  })
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

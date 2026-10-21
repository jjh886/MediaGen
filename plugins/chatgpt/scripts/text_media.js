function chatgptApiConsumeRichTextEvent(state, writer, event) {
  if (!event || typeof event !== 'object') return
  if (event.type === 'server_ste_metadata' && event.metadata?.turn_use_case === 'image gen') {
    state.image_pending = true
    apiTextImageProgress(writer, 'turn', 'in_progress')
  }
  const value = event.v && !Array.isArray(event.v) && typeof event.v === 'object' ? event.v : event
  if (value.message) state.rich_message = JSON.parse(JSON.stringify(value.message))
  const operations = Array.isArray(event.v) ? event.v : typeof event.p === 'string' ? [event] : []
  for (const operation of operations) {
    if (operation.p === '/message') state.rich_message = JSON.parse(JSON.stringify(operation.v))
    else if (operation.p?.startsWith('/message/') && state.rich_message)
      chatgptApiPatchRichMessage(state.rich_message, operation)
  }
  const message = state.rich_message
  if (
    !message ||
    message.author?.role === 'user' ||
    message.metadata?.is_visually_hidden_from_conversation
  )
    return
  const content = message.content || {}
  if (content.content_type === 'thoughts') {
    const text = (content.thoughts || [])
      .map((thought) => [thought.summary, thought.content].filter(Boolean).join('\n'))
      .filter(Boolean)
      .join('\n\n')
    if (text)
      apiTextUpdateReasoning(
        writer,
        message.id,
        text,
        message.metadata?.reasoning_status === 'reasoning_ended'
      )
  } else if (content.content_type === 'reasoning_recap') {
    for (const id of Object.keys(writer.reasoning_items || {}))
      apiTextUpdateReasoning(writer, id, '', true)
  }
  const ghost = message.metadata?.ghostrider
  if (ghost || (content.parts || []).some((part) => part?.content_type === 'image_asset_pointer')) {
    state.image_pending = true
    apiTextImageProgress(writer, 'turn', 'generating')
    state.image_messages ||= {}
    state.image_messages[message.id] = message
  }
}

function chatgptApiPatchRichMessage(message, operation) {
  const keys = operation.p
    .slice('/message/'.length)
    .split('/')
    .map((key) => key.replace(/~1/g, '/').replace(/~0/g, '~'))
  if (keys.some((key) => ['__proto__', 'constructor', 'prototype'].includes(key))) return
  let target = message
  for (const key of keys.slice(0, -1)) {
    if (!target[key] || typeof target[key] !== 'object') target[key] = {}
    target = target[key]
  }
  const key = keys.at(-1)
  if (operation.o === 'append') {
    if (typeof target[key] === 'string') target[key] += String(operation.v || '')
    else if (Array.isArray(target[key]))
      target[key].push(...(Array.isArray(operation.v) ? operation.v : [operation.v]))
  } else if (operation.o === 'remove') delete target[key]
  else target[key] = operation.v
}

function chatgptApiCurrentTurnMessages(conversation, userId, baseline) {
  if (Array.isArray(conversation.messages)) {
    let start = userId ? conversation.messages.findIndex((message) => message.id === userId) : -1
    if (!userId && baseline === 'client-created-root') {
      for (let index = conversation.messages.length - 1; index >= 0; index--) {
        if (conversation.messages[index].author?.role === 'user') {
          start = index
          break
        }
      }
    }
    return start >= 0 ? conversation.messages.slice(start + 1) : []
  }
  const mapping = conversation.mapping || {}
  let id = conversation.current_node
  const messages = []
  const visited = new Set()
  while (id && mapping[id] && !visited.has(id)) {
    if (id === userId || id === baseline) return messages.reverse()
    visited.add(id)
    const node = mapping[id]
    if (node.message?.author?.role === 'user')
      return node.message.id === userId || (!userId && baseline === 'client-created-root')
        ? messages.reverse()
        : []
    if (node.message) messages.push(node.message)
    id = node.parent
  }
  return !userId && baseline === 'client-created-root' ? messages.reverse() : []
}

function chatgptApiResolveTextImages(auth, conversationId, state, writer) {
  state.completed_files ||= {}
  state.pending_image_files = false
  let count = Object.keys(state.completed_files).length
  for (const message of Object.values(state.image_messages || {})) {
    if (message.metadata?.ghostrider && message.metadata.ghostrider.status !== 'final') continue
    for (const part of message.content?.parts || []) {
      if (part?.content_type !== 'image_asset_pointer') continue
      const fileId = /^(?:sediment|file-service):\/\/(file[-_][a-zA-Z0-9_-]+)$/.exec(
        part.asset_pointer || ''
      )?.[1]
      if (!fileId || state.completed_files[fileId]) continue
      const path = '/backend-api/files/download/' + encodeURIComponent(fileId)
      const response = chatgptApiRequest(
        auth,
        'GET',
        path +
          '?conversation_id=' +
          encodeURIComponent(conversationId) +
          '&download_intent=false&inline=true&include_library_file_state=true',
        {
          target_path: path,
          target_route: '/backend-api/files/download/{file_id}',
          timeout_ms: 60000,
        }
      )
      if ([404, 409, 425].includes(Number(response.status))) {
        state.pending_image_files = true
        continue
      }
      const info = chatgptApiRequireOk(response, '获取对话生成图片').body
      const url = info?.download_url || info?.url
      if (!url) {
        state.pending_image_files = true
        continue
      }
      const data = chatgptApiDownloadImageData(auth, url)
      if (!data) {
        state.pending_image_files = true
        continue
      }
      apiTextCompleteImage(writer, count === 0 ? 'turn' : fileId, data, {
        width: part.width,
        height: part.height,
      })
      state.completed_files[fileId] = true
      count++
    }
  }
  return count
}

function chatgptApiWaitForRichText(auth, conversationId, parentId, state, writer, connection) {
  const deadline = Date.now() + CHATGPT_API_TEXT_TIMEOUT_MS
  const saved = chatgptApiCheckpoint()
  // 固定视频跨重启保留等待预算；仍须实际读到原会话后才判断超时。
  // 本机排队不代表上游失败，不能仅因恢复次数增加而重投。
  const recordedStart = saved.fixed_video_wait_started_at_ms ?? saved.history_probe?.checked_at_ms
  const waitStarted =
    Number.isFinite(recordedStart) && recordedStart <= Date.now() ? recordedStart : Date.now()
  const videoDeadline = waitStarted + CHATGPT_API_TEXT_TIMEOUT_MS
  if (chatgptApiFixedVideoAnalysis)
    chatgptApiCheckpoint({ ...saved, fixed_video_wait_started_at_ms: waitStarted })
  let freshHistoryRead = false
  let delay = chatgptApiClampPollDelay(saved.poll_retry_ms || CHATGPT_API_TEXT_POLL_MS, 3600000)
  let nextPoll = connection
    ? Math.max(Number(saved.poll_next_at_ms || 0), Date.now() + CHATGPT_API_TEXT_POLL_MS)
    : saved.poll_rate_limited === false
      ? Date.now()
      : Math.max(Number(saved.poll_next_at_ms || 0), Date.now())
  const initialConnection = connection
  let connected = !!connection
  const userId = saved.user_message_id || ''
  try {
    while (Date.now() < deadline) {
      if (Date.now() >= nextPoll) {
        const response = chatgptApiTextConversationResponse(auth, conversationId)
        if (chatgptApiMissingConversation(response)) {
          nextPoll = Date.now() + delay
          host.call('time.sleep', { ms: 1000 })
          continue
        }
        chatgptApiRequireRetryableRead(response, conversationId)
        if (chatgptApiPollRateLimited(response)) {
          delay = chatgptApiPollRetryDelayMs(response, delay)
          nextPoll = Date.now() + delay
          host.checkpoint({
            ...host.checkpoint(),
            poll_retry_ms: delay,
            poll_next_at_ms: nextPoll,
            poll_rate_limited: true,
            poll_rate_limit_source: 'local',
          })
          host.call('time.sleep', { ms: 1000 })
          continue
        }
        const conversation = chatgptApiRequireOk(response, '读取 ChatGPT 对话结果').body || {}
        freshHistoryRead = true
        const error = chatgptApiTextConversationError(conversation)
        if (error) chatgptApiFailConversation('upstream_failed')
        const messages = chatgptApiCurrentTurnMessages(conversation, userId, parentId)
        for (const message of messages)
          chatgptApiConsumeTextStreamEvent(state, writer, {
            message,
            conversation_id: conversationId,
          })
        if (
          messages.some((message) =>
            ['failed', 'error', 'cancelled', 'canceled'].includes(message.status)
          )
        )
          chatgptApiFailConversation('upstream_failed')
        const final = messages
          .filter(
            (message) =>
              !message.metadata?.is_visually_hidden_from_conversation &&
              message.author?.role === 'assistant' &&
              (!message.channel || message.channel === 'final') &&
              chatgptApiTextMessageFinished(message) &&
              message.content?.content_type === 'text'
          )
          .at(-1)
        if (final) {
          // 后续工具/摘要事件不应抹掉已经确认完成的最终正文。
          chatgptApiConsumeTextStreamEvent(state, writer, {
            message: { ...final, channel: final.channel || 'final' },
            conversation_id: conversationId,
          })
          state.final_text = chatgptApiTextParts(final.content.parts)
        }
        delay = CHATGPT_API_TEXT_POLL_MS
        nextPoll = Date.now() + delay
        host.checkpoint({
          ...host.checkpoint(),
          poll_retry_ms: delay,
          poll_next_at_ms: nextPoll,
          poll_rate_limited: false,
          history_probe: chatgptApiHistoryProbe(conversation, userId, messages),
        })
      }
      const images = chatgptApiResolveTextImages(auth, conversationId, state, writer)
      if (
        (images && !state.pending_image_files) ||
        (!state.image_pending &&
          state.finished &&
          chatgptApiIsFinalText(state) &&
          state.final_text?.trim())
      ) {
        for (const id of Object.keys(writer.reasoning_items || {}))
          apiTextUpdateReasoning(writer, id, '', true)
        return state.final_text || ''
      }
      if (chatgptApiFixedVideoAnalysis && freshHistoryRead && Date.now() >= videoDeadline)
        chatgptApiFailConversation('generation_timeout')
      if (!connected) {
        connected = true
        connection = chatgptApiTryOpenTextUpdates(auth)
      }
      if (connection) {
        const open = chatgptApiReceiveTextUpdates(connection, conversationId, (event) => {
          chatgptApiConsumeTextStreamEvent(state, writer, event)
        })
        if (open === false) {
          chatgptApiCloseTextUpdates(connection)
          connection = null
          // 断线后立即读取原会话；已有 429 退避时仍遵守服务器要求的等待时间。
          if (!host.checkpoint()?.poll_rate_limited) nextPoll = Date.now()
        }
      } else {
        host.call('time.sleep', { ms: 1000 })
      }
    }
    const last = chatgptApiCheckpoint()
    if (last.poll_rate_limited && last.poll_rate_limit_source === 'local')
      throw new Error('temporary_unavailable: 本机查询仍在排队，继续原任务')
    chatgptApiFailConversation('generation_timeout')
  } finally {
    if (connection !== initialConnection) chatgptApiCloseTextUpdates(connection)
  }
}

// 只保存历史响应的结构特征，便于判断恢复停在哪一步；不记录正文或凭据。
function chatgptApiHistoryProbe(conversation, userId, currentMessages) {
  const nodes = Array.isArray(conversation.messages)
    ? conversation.messages
    : Object.values(conversation.mapping || {})
        .map((node) => node.message)
        .filter(Boolean)
  const first = nodes[0] || {}
  return {
    checked_at_ms: Date.now(),
    root_keys: Object.keys(conversation),
    async_status: conversation.async_status,
    page_info: Object.fromEntries(
      Object.entries(conversation.page_info || {}).map(([key, value]) => [
        key,
        typeof value === 'boolean' || typeof value === 'number' || value === null
          ? value
          : { type: typeof value, present: Boolean(value) },
      ])
    ),
    message_count: nodes.length,
    current_turn_messages: currentMessages.length,
    requested_user_found: nodes.some((message) => message.id === userId),
    first_message_keys: Object.keys(first),
    roles: [...new Set(nodes.map((message) => message.author?.role || message.role || 'unknown'))],
    states: [...new Set(nodes.map((message) => message.status || 'unknown'))],
    content_types: [...new Set(nodes.map((message) => message.content?.content_type || 'unknown'))],
    completed_assistant_count: nodes.filter(
      (message) => message.author?.role === 'assistant' && chatgptApiTextMessageFinished(message)
    ).length,
  }
}

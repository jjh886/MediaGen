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
    const start = conversation.messages.findIndex((message) => message.id === userId)
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
      return node.message.id === userId ? messages.reverse() : []
    if (node.message) messages.push(node.message)
    id = node.parent
  }
  return baseline === 'client-created-root' ? messages.reverse() : []
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
  let delay = chatgptApiClampPollDelay(saved.poll_retry_ms || CHATGPT_API_TEXT_POLL_MS, 3600000)
  let nextPoll = Math.max(Number(saved.poll_next_at_ms || 0), Date.now() + CHATGPT_API_TEXT_POLL_MS)
  const userId = saved.user_message_id || ''
  while (Date.now() < deadline) {
    chatgptApiReceiveTextUpdates(connection, conversationId, (event) => {
      chatgptApiConsumeTextStreamEvent(state, writer, event)
    })
    if (Date.now() >= nextPoll) {
      const response = chatgptApiTextConversationResponse(auth, conversationId)
      if (chatgptApiPollRateLimited(response)) {
        delay = chatgptApiPollRetryDelayMs(response, delay)
        nextPoll = Date.now() + delay
        host.checkpoint({ ...host.checkpoint(), poll_retry_ms: delay, poll_next_at_ms: nextPoll })
        continue
      }
      const conversation = chatgptApiRequireOk(response, '读取 ChatGPT 对话结果').body || {}
      const error = chatgptApiTextConversationError(conversation)
      if (error) throw new Error(error)
      const messages = chatgptApiCurrentTurnMessages(conversation, userId, parentId)
      for (const message of messages)
        chatgptApiConsumeTextStreamEvent(state, writer, {
          message,
          conversation_id: conversationId,
        })
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
      if (final) state.final_text = chatgptApiTextParts(final.content.parts)
      delay = CHATGPT_API_TEXT_POLL_MS
      nextPoll = Date.now() + delay
      host.checkpoint({ ...host.checkpoint(), poll_retry_ms: delay, poll_next_at_ms: nextPoll })
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
  }
  throw new Error('generation_timeout: ChatGPT 对话生成超过 20 分钟')
}

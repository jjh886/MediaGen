// 网页先注册用户 WebSocket，再发送普通会话；平台路由与消息解释只在 JS 中维护。
function chatgptApiOpenTextUpdates(auth) {
  const path = '/backend-api/celsius/ws/user'
  const registration = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'GET', path, {
      target_path: path,
      target_route: path,
      timeout_ms: 30000,
    }),
    '连接 ChatGPT 消息通知'
  ).body
  if (
    typeof registration?.websocket_url !== 'string' ||
    !registration.websocket_url.startsWith('wss://ws.chatgpt.com/')
  )
    throw new Error('temporary_unavailable: ChatGPT 消息连接地址无效')
  const connection = host.call('http.websocket_open', {
    url: registration.websocket_url,
    proxy: 'system',
    headers: {
      'user-agent': chatgptApiFingerprint(auth).user_agent,
      origin: CHATGPT_API_ORIGIN,
      'accept-language': chatgptApiFingerprint(auth).accept_language || 'zh-CN,zh;q=0.9',
      'cache-control': 'no-cache',
      pragma: 'no-cache',
    },
  })
  try {
    host.call('http.websocket_send', {
      connection_id: connection.connection_id,
      text: JSON.stringify([
        {
          command: { presence: { state: 'foreground', type: 'presence' }, type: 'connect' },
          id: 1,
        },
        { command: { topic_id: 'pages', type: 'subscribe' }, id: 2 },
      ]),
    })
    return connection
  } catch (error) {
    chatgptApiCloseTextUpdates(connection)
    throw error
  }
}

function chatgptApiCloseTextUpdates(connection) {
  if (connection?.connection_id)
    host.tryCall('http.websocket_close', { connection_id: connection.connection_id })
}

function chatgptApiReceiveTextUpdates(connection, conversationId, consume) {
  let result
  try {
    result = host.call('http.websocket_receive', {
      connection_id: connection.connection_id,
      timeout_ms: 1000,
    })
  } catch (error) {
    // 原子能力在异常断线时抛错，正常关闭则返回 closed；两者都转为原会话轮询。
    // 仅捕获连接传输错误，不能吞掉权限检查或下方消息处理的错误。
    if (!/WebSocket (?:读取失败|心跳失败|连接已关闭)/i.test(String(error))) throw error
    return false
  }
  if (result.closed) return false
  if (!result.text) return
  let event
  try {
    event = JSON.parse(result.text)
  } catch {
    return
  }
  for (const item of Array.isArray(event) ? event : [event]) {
    if (item.type === 'conversation-update' && item.payload?.conversation_id === conversationId) {
      for (const message of item.payload.update_content?.messages || [])
        consume({ message, conversation_id: conversationId })
    }
  }
}

function chatgptApiTryOpenTextUpdates(auth) {
  try {
    return chatgptApiOpenTextUpdates(auth)
  } catch (error) {
    if (
      !/temporary_unavailable|timeout|connection|websocket|TLS|消息连接|HTTP (429|5\d\d)/i.test(
        String(error)
      )
    )
      throw error
    return null
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
  if (event.p === '' && event.v?.message) {
    chatgptApiApplyFullTextMessage(state, event.v)
    return
  }
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

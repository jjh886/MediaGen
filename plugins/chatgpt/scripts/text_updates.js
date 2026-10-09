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
  const result = host.call('http.websocket_receive', {
    connection_id: connection.connection_id,
    timeout_ms: 1000,
  })
  if (result.closed) throw new Error('temporary_unavailable: ChatGPT 消息连接已断开，请恢复原会话')
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

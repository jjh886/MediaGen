const CHATGPT_API_FALLBACK_POLL_MS = 30000

// 正常链路直接消费 SSE；只有 SSE 缺少最终图片时才低频查询会话作为容错。
// 查询接口的 429 只代表“暂时不允许继续查询”，不能判定生成任务失败。
function chatgptApiPollRateLimited(response) {
  return Number(response?.status || 0) === 429
}

function chatgptApiPollRetryDelayMs(response, previousDelayMs) {
  const header = Object.entries(response?.headers || {}).find(
    ([name]) => name.toLowerCase() === 'retry-after'
  )
  const raw = String(header?.[1] || '').trim()
  const seconds = Number(raw)
  if (raw && Number.isFinite(seconds)) return chatgptApiClampPollDelay(seconds * 1000, 3600000)
  const timestamp = Date.parse(raw)
  if (raw && Number.isFinite(timestamp)) {
    return chatgptApiClampPollDelay(timestamp - Date.now(), 3600000)
  }
  return chatgptApiClampPollDelay(
    Math.max(CHATGPT_API_FALLBACK_POLL_MS, Number(previousDelayMs || 0) * 2)
  )
}

function chatgptApiClampPollDelay(value, maximum = 300000) {
  return Math.min(maximum, Math.max(CHATGPT_API_FALLBACK_POLL_MS, Math.ceil(Number(value) || 0)))
}

// 新网页会将生成转交给 Conduit。只接续原会话，不重新提交用户消息。
function chatgptApiStreamWithResume(auth, request, onEvent) {
  const deadline = Date.now() + (request.timeout_ms || 3 * 60 * 60 * 1000)
  let next = request
  let result
  let token = ''
  let conversationId = ''
  do {
    let handoff = false
    let completed = false
    result = host.stream('http.sse_request', next, (event) => {
      if (event?.type === 'resume_conversation_token') {
        token = typeof event.token === 'string' ? event.token : ''
        conversationId = String(event.conversation_id || '')
      }
      if (event?.type === 'stream_handoff') handoff = true
      if (event?.type === 'message_stream_complete') completed = true
      onEvent(event)
      if (event?.type === 'resume_conversation_token' && token && conversationId) {
        chatgptApiCheckpoint({
          ...chatgptApiCheckpoint(),
          phase: 'accepted',
          conversation_id: conversationId,
          resume_token: token,
          resume_kind: event.kind || '',
        })
      }
      return null
    })
    chatgptApiRememberCookies(auth, result, next.url.slice(CHATGPT_API_ORIGIN.length).split('?')[0])
    if (!result.ok || !handoff || completed) return result
    if (!conversationId || !token) {
      throw new Error('PROVIDER_SUBMISSION_UNKNOWN: ChatGPT 转交结果缺少恢复凭据，请恢复原任务')
    }
    const remaining = deadline - Date.now()
    if (remaining <= 0) throw new Error('temporary_unavailable: ChatGPT 接续结果超时')
    next = chatgptApiResumeRequest(auth, conversationId, token, remaining)
  } while (true)
}

function chatgptApiResumeRequest(auth, conversationId, token, timeoutMs) {
  const path = '/backend-api/f/conversation/resume'
  return {
    method: 'POST',
    url: CHATGPT_API_ORIGIN + path,
    proxy: 'system',
    tls_profile: chatgptApiTlsProfile(auth),
    headers: {
      ...chatgptApiBaseHeaders(auth, path, path),
      accept: 'text/event-stream',
      'content-type': 'application/json',
      'x-conduit-token': token,
    },
    body: { conversation_id: conversationId, offset: 0 },
    retry: { max_attempts: 1 },
    timeout_ms: timeoutMs,
  }
}

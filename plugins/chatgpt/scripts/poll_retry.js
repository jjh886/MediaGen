const CHATGPT_API_FALLBACK_POLL_MS = 30000

// 正常链路直接消费 SSE；只有 SSE 缺少最终图片时才低频查询会话作为容错。
// 上游真实 429 按业务规则结束本轮；宿主合并/排队返回的本地等待不代表平台任务失败。
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
    chatgptApiRequireRetryableRead(result)
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

// 404 可能是短暂索引延迟；连续确认不存在后放弃旧编号，允许下一轮重新提交。
function chatgptApiMissingConversation(response) {
  const status = Number(response?.status || 0)
  const saved = host.checkpoint() || {}
  if (![404, 410].includes(status)) {
    if (saved.missing_conversation_count)
      host.checkpoint({ ...saved, missing_conversation_count: 0 })
    return false
  }
  const count = Number(saved.missing_conversation_count || 0) + 1
  if (status === 410 || count >= 3) {
    host.checkpoint({
      phase: 'ready',
      images: saved.images || [],
      references: saved.references || [],
    })
    throw new Error('temporary_unavailable: 原 ChatGPT 会话已无法查询，将重新提交请求')
  }
  host.checkpoint({ ...saved, missing_conversation_count: count })
  return true
}

function chatgptApiRetryCheckpoint(reason, delayMs = CHATGPT_API_FALLBACK_POLL_MS) {
  const saved = chatgptApiCheckpoint()
  const value = {
    phase: 'retry_pending',
    reset_session: true,
    retry_reason: reason,
    retry_not_before_ms: Date.now() + Math.max(0, delayMs),
    failed_conversation_id: saved.conversation_id || saved.failed_conversation_id || '',
    images: saved.images || [],
    references: saved.references || [],
    ...(Array.isArray(saved.attachments) ? { attachments: saved.attachments } : {}),
  }
  chatgptApiCheckpoint(value)
  return value
}

function chatgptApiFailConversation(reason, delayMs) {
  chatgptApiRetryCheckpoint(reason, delayMs)
  throw new Error('temporary_unavailable: 原任务已失败，将新建会话重试：' + reason)
}

function chatgptApiPrepareConversationAttempt() {
  let saved = chatgptApiCheckpoint()
  if (saved.phase === 'completed') return saved
  if (
    saved.phase === 'failed' ||
    ((saved.poll_rate_limited ||
      (saved.poll_rate_limited === undefined && Number(saved.poll_next_at_ms || 0) > 0)) &&
      saved.poll_rate_limit_source !== 'local')
  ) {
    saved = chatgptApiRetryCheckpoint(
      saved.phase === 'failed' ? 'upstream_failed' : 'upstream_rate_limited',
      Math.max(0, Number(saved.poll_next_at_ms || 0) - Date.now())
    )
  }
  if (saved.phase !== 'retry_pending') return saved
  const remaining = Number(saved.retry_not_before_ms || 0) - Date.now()
  if (remaining > 0)
    throw new Error(
      'temporary_unavailable: 原任务已失败，新会话重试等待 ' + Math.ceil(remaining / 1000) + ' 秒'
    )
  const ready = {
    phase: 'ready',
    reset_session: true,
    images: saved.images,
    references: saved.references,
    ...(Array.isArray(saved.attachments) ? { attachments: saved.attachments } : {}),
  }
  chatgptApiCheckpoint(ready)
  return ready
}

function chatgptApiRequireRetryableRead(response) {
  if (!chatgptApiPollRateLimited(response) || response.coalesced === true) return
  chatgptApiFailConversation('upstream_rate_limited', chatgptApiPollRetryDelayMs(response))
}

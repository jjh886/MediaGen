const CHATGPT_API_FALLBACK_POLL_MS = 30000

// 正常链路直接消费 SSE；只有 SSE 缺少最终图片时才低频查询会话作为容错。
// 查询接口的 429 只代表“暂时不允许继续查询”，不能判定生成任务失败。
function chatgptApiPollRateLimited(response) {
  return Number(response?.status || 0) === 429
}

function chatgptApiPollRetryDelayMs(response, previousDelayMs) {
  const raw = String(response?.headers?.['retry-after'] || '').trim()
  const seconds = Number(raw)
  if (raw && Number.isFinite(seconds)) return chatgptApiClampPollDelay(seconds * 1000)
  const timestamp = Date.parse(raw)
  if (raw && Number.isFinite(timestamp)) {
    return chatgptApiClampPollDelay(timestamp - Date.now())
  }
  return chatgptApiClampPollDelay(
    Math.max(CHATGPT_API_FALLBACK_POLL_MS, Number(previousDelayMs || 0) * 2)
  )
}

function chatgptApiClampPollDelay(value) {
  return Math.min(60000, Math.max(CHATGPT_API_FALLBACK_POLL_MS, Math.ceil(Number(value) || 0)))
}

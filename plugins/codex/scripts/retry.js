const RETRY_DELAYS_MS = [5000, 15000, 30000, 60000]
const DEFAULT_MAX_ATTEMPTS = 3

function formatOpenAiError(error) {
  const message = compactError(error)
  const body = parseJsonErrorBody(message)
  if (!body) return message

  const code = firstNonEmpty([get(body, ['error', 'code']), body.code])
  const detail = firstNonEmpty([get(body, ['error', 'message']), body.message])
  const type = firstNonEmpty([get(body, ['error', 'type']), body.type])
  const prefix = httpStatusPrefix(message)
  return [prefix, code, type && type !== code ? type : '', detail || message]
    .filter(Boolean)
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim()
}

function shouldRetryMessage(message) {
  const text = compactError(message).toLowerCase()
  if (isPermanentFailure(text)) return false
  return retryPatterns().some((pattern) => text.includes(pattern))
}

function isPermanentFailure(text) {
  // 鉴权、参数和安全策略类错误不是网络抖动，重试只会浪费账号额度。
  return [
    'safety_violations',
    'safety system',
    'bad request',
    'unsupported',
    'invalid',
    'unknown_error',
    'request_forbidden',
    'forbidden',
    'http 400',
    'http 401',
    'http 403',
    'http 404',
  ].some((pattern) => text.includes(pattern))
}

function retryPatterns() {
  return [
    'server_error',
    'retryable error',
    'you can retry',
    'timeout',
    'timed out',
    'error decoding response body',
    'decoding response body',
    'unexpected eof',
    'incomplete message',
    'error sending request',
    'connection closed',
    'connection reset',
    'connection refused',
    'econnreset',
    'econnrefused',
    'enotfound',
    'eai_again',
    'socket hang up',
    'proxy',
    'tunnel',
    'network',
    'rate limit',
    'overloaded',
    'http 429',
    'http 500',
    'http 502',
    'http 503',
    'http 504',
  ]
}

function canRetry(attempt, retryable) {
  return retryable && attempt < DEFAULT_MAX_ATTEMPTS
}

function sleepBeforeRetry(attempt) {
  host.sleep(RETRY_DELAYS_MS[Math.min(attempt - 1, RETRY_DELAYS_MS.length - 1)])
}

function compactError(error) {
  return String((error && error.message) || error)
    .replace(/\s+/g, ' ')
    .trim()
}

function parseJsonErrorBody(message) {
  const text = String(message || '')
  const start = text.indexOf('{')
  const end = text.lastIndexOf('}')
  if (start < 0 || end < start) return null
  try {
    return JSON.parse(text.slice(start, end + 1))
  } catch (_) {
    return null
  }
}

function httpStatusPrefix(message) {
  const match = String(message || '').match(/HTTP\s+\d+/i)
  return match ? match[0].toUpperCase() : ''
}

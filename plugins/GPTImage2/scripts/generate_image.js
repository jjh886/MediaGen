const ENDPOINT = 'https://chatgpt.com/backend-api/codex/responses'
const MODEL = 'gpt-5.5'
const RETRY_DELAYS_MS = [5000, 15000, 30000, 60000]
const DEFAULT_MAX_ATTEMPTS = 1
const INSTRUCTIONS =
  "Use the image_generation tool to create the requested image. Do not rewrite, expand, polish, or add creative details to the user's prompt. Preserve the user intent, exact text, subject, style, composition, and constraints. Return the image generation result."

function main(ctx) {
  const auth = host.call('file.read_json', { path: ctx.profile_dir + '/auth.json' })
  const accessToken = get(auth, ['tokens', 'access_token'])
  if (!accessToken) throw new Error('auth.json 缺少 tokens.access_token，请重新登录 GPTImage2。')
  const request = buildRequest(ctx)
  const accountId = firstNonEmpty([
    get(auth, ['tokens', 'account_id']),
    accountIdFromToken(accessToken),
  ])
  const images = []

  for (let index = 0; index < request.count; index++) {
    const prompt = promptForIndex(request.prompt, index, request.count)
    const payload = buildPayload(ctx, prompt, request.size)
    const result = requestImageWithRetry(accessToken, accountId, payload)
    const dimensions = dimensionsFromPng(result.image)
    images.push({
      url: 'data:image/png;base64,' + result.image,
      width: dimensions.width,
      height: dimensions.height,
      revised_prompt: result.revised_prompt || null,
      usage: result.usage || null,
    })
  }

  return {
    success: true,
    status: 'done',
    source: 'GPTImage2',
    imageCount: images.length,
    images,
  }
}

function buildRequest(ctx) {
  const prompt = String(ctx.prompt || '').trim()
  if (!prompt) throw new Error('Missing prompt.')
  return {
    prompt,
    count: clampNumber(ctx.max_images || 1, 1, 4),
    size: resolveSize(ctx),
  }
}

function promptForIndex(prompt, index, count) {
  return count > 1 ? prompt + '\n\nCreate variation ' + (index + 1) + ' of ' + count + '.' : prompt
}

function buildPayload(ctx, prompt, size) {
  const content = []
  const refs = Array.isArray(ctx.ref_image_locals) ? ctx.ref_image_locals : []
  for (const path of refs) {
    if (!path) continue
    const file = host.call('file.read_base64', {
      path,
      max_bytes: 50 * 1024 * 1024,
    })
    content.push({
      type: 'input_text',
      text: 'The next image is a reference image. Use it only for identity, style, composition, or visual guidance.',
    })
    content.push({
      type: 'input_image',
      image_url: 'data:' + file.mime + ';base64,' + file.base64,
    })
  }
  content.push({ type: 'input_text', text: prompt || '' })

  return {
    model: MODEL,
    instructions: INSTRUCTIONS,
    input: [{ role: 'user', content }],
    reasoning: { effort: 'low' },
    tools: [
      {
        type: 'image_generation',
        size,
        quality: 'high',
        background: 'auto',
        output_format: 'png',
        action: 'generate',
      },
    ],
    tool_choice: { type: 'image_generation' },
    store: false,
    stream: true,
  }
}

function requestImageWithRetry(accessToken, accountId, payload) {
  for (let attempt = 1; attempt <= DEFAULT_MAX_ATTEMPTS; attempt++) {
    try {
      const result = requestImage(accessToken, accountId, payload)
      if (result.image) return result
      if (!canRetry(attempt, shouldRetryMessage(result.error))) throw new Error(resultError(result))
      sleepBeforeRetry(attempt)
    } catch (error) {
      const message = formatOpenAiError(error)
      if (!canRetry(attempt, shouldRetryMessage(message))) throw new Error(message)
      sleepBeforeRetry(attempt)
    }
  }
  throw new Error('No image_generation_call.result found.')
}

function requestImage(accessToken, accountId, payload) {
  const headers = {
    authorization: 'Bearer ' + accessToken,
    'content-type': 'application/json',
    accept: 'text/event-stream',
  }
  if (accountId) headers['chatgpt-account-id'] = accountId
  const sse = host.call('http.sse_request', {
    method: 'POST',
    url: ENDPOINT,
    proxy: 'system',
    headers,
    body: payload,
    timeout_ms: 1800000,
  })
  const result = extractImage(sse.events || [])
  return result
}

function extractImage(events) {
  const out = { image: '', revised_prompt: '', usage: null, error: '', error_code: '', types: [] }
  const types = []
  for (const event of events) {
    collectEventType(types, event)
    applyEvent(out, event)
  }
  if (!out.image && !out.error && types.length) {
    out.error = 'No image_generation_call.result found. events: ' + types.join(', ')
  }
  out.types = types
  return out
}

function collectEventType(types, event) {
  if (event && event.type) types.push(event.type)
}

function applyEvent(out, event) {
  if (!event) return
  if (event.type === 'error') applyError(out, event)
  if (event.type === 'response.failed') applyError(out, get(event, ['response', 'error']) || {})
  if (event.type === 'response.output_item.done') applyImageCall(out, event.item || {})
  if (event.type === 'response.completed') applyCompletedResponse(out, event.response || {})
}

function applyCompletedResponse(out, response) {
  if (response.usage) out.usage = response.usage
  if (response.error) applyError(out, response.error)
  if (!Array.isArray(response.output)) return
  for (const item of response.output) applyImageCall(out, item)
}

function applyImageCall(out, item) {
  if (!item || item.type !== 'image_generation_call') return
  if (item.result) out.image = item.result
  if (item.revised_prompt) out.revised_prompt = item.revised_prompt
}

function applyError(out, error) {
  if (!error) return
  const message = error.message || error.error || error.code || ''
  if (!message) return
  const code = error.code || error.type || ''
  out.error_code = code
  out.error = code && code !== message ? code + ': ' + message : message
}

function resultError(result) {
  if (result.error) return result.error
  const types = unique(result.types || []).join(', ')
  return 'No image_generation_call.result found.' + (types ? ' events: ' + types : '')
}

function compactError(error) {
  return String((error && error.message) || error)
    .replace(/\s+/g, ' ')
    .trim()
}

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

function shouldRetryMessage(message) {
  const text = compactError(message).toLowerCase()
  if (isPermanentFailure(text)) return false
  return retryPatterns().some((pattern) => text.includes(pattern))
}

function isPermanentFailure(text) {
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
    'econnreset',
    'econnrefused',
    'enotfound',
    'eai_again',
    'socket hang up',
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

function accountIdFromToken(token) {
  const payload = decodeJwtPayload(token)
  return firstNonEmpty([
    get(payload, ['https://api.openai.com/auth', 'account_id']),
    get(payload, ['https://api.openai.com/auth', 'chatgpt_account_id']),
  ])
}

function decodeJwtPayload(token) {
  const parts = String(token || '').split('.')
  if (parts.length < 2) return {}
  return JSON.parse(host.call('crypto.base64url_decode_utf8', { text: parts[1] }).value)
}

function resolveSize(ctx) {
  const requestedSize = String(ctx.size || ctx.image_size || '').trim()
  if (/^\d+x\d+$/.test(requestedSize)) return requestedSize
  const ratio = String(ctx.image_aspect_ratio || requestedSize || '9:16')
  return sizeForRatio(ratio)
}

function sizeForRatio(ratio) {
  if (ratio === '16:9') return '2560x1440'
  if (ratio === '1:1') return '2048x2048'
  if (ratio === '3:4') return '1536x2048'
  if (ratio === '4:3') return '2048x1536'
  return '1440x2560'
}

function dimensionsFromPng(imageBase64) {
  const bytes = base64ToBytes(imageBase64)
  const isPng = bytes.length >= 24 && bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e
  if (!isPng) return { width: 0, height: 0 }
  return {
    width: readUInt32BE(bytes, 16),
    height: readUInt32BE(bytes, 20),
  }
}

function base64ToBytes(text) {
  const clean = String(text || '').replace(/[^A-Za-z0-9+/=]/g, '')
  const out = []
  for (let i = 0; i < clean.length; i += 4) {
    pushBase64Chunk(out, clean.slice(i, i + 4))
  }
  return out
}

function pushBase64Chunk(out, chunk) {
  const n =
    (base64Value(chunk.charAt(0)) << 18) |
    (base64Value(chunk.charAt(1)) << 12) |
    (base64Value(chunk.charAt(2)) << 6) |
    base64Value(chunk.charAt(3))
  out.push((n >> 16) & 255)
  if (chunk.charAt(2) !== '=') out.push((n >> 8) & 255)
  if (chunk.charAt(3) !== '=') out.push(n & 255)
}

function base64Value(ch) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
  const index = alphabet.indexOf(ch)
  return index < 0 ? 0 : index
}

function readUInt32BE(bytes, offset) {
  return (
    bytes[offset] * 16777216 +
    bytes[offset + 1] * 65536 +
    bytes[offset + 2] * 256 +
    bytes[offset + 3]
  )
}

function clampNumber(value, min, max) {
  const n = Number(value)
  if (!Number.isFinite(n)) return min
  return Math.max(min, Math.min(max, Math.floor(n)))
}

function firstNonEmpty(values) {
  for (const value of values) {
    if (typeof value === 'string' && value.trim()) return value
  }
  return ''
}

function unique(values) {
  const out = []
  for (const value of values) {
    if (out.indexOf(value) === -1) out.push(value)
  }
  return out
}

function get(value, path) {
  let current = value
  for (const key of path) {
    if (!current || typeof current !== 'object') return ''
    current = current[key]
  }
  return current
}

const MAX_UPSTREAM_ERROR_LENGTH = 2000

function applyHttpError(out, response) {
  const detail = get(response, ['body', 'error']) || response.body || response.body_text || {}
  applyError(out, detail)
  const status = Number(response.status || 0)
  if (status && out.error) out.error = 'HTTP ' + status + ' ' + out.error
}

function extractImage(events) {
  const out = {
    image: '',
    revised_prompt: '',
    usage: null,
    error: '',
    error_code: '',
    output_text: '',
    types: [],
    partial_images: [],
  }
  const types = []
  for (const event of events) {
    collectEventType(types, event)
    applyEvent(out, event)
  }
  if (!out.image && !out.error && types.length) {
    const detail = compactUpstreamText(out.output_text)
    out.error_code = stableProviderErrorCode('', detail)
    out.error = detail
      ? out.error_code
        ? out.error_code + ': ' + detail
        : 'Codex API 返回文本：' + detail
      : missingImageError(types)
  }
  out.types = types
  return out
}

function collectEventType(types, event) {
  if (event && event.type) types.push(event.type)
}

function applyEvent(out, event) {
  if (!event) return
  applyFailureEvent(out, event)
  applyTextEvent(out, event)
  applyImageEvent(out, event)
}

// 分开处理三类 SSE 事件，避免平台新增事件时把同一个分派函数越堆越复杂。
function applyFailureEvent(out, event) {
  if (event.type === 'error') applyError(out, event)
  if (event.type === 'response.failed') applyError(out, get(event, ['response', 'error']) || {})
  if (event.type !== 'response.failed' && String(event.type || '').endsWith('.failed')) {
    applyError(out, event.error || get(event, ['item', 'error']) || event)
  }
}

function applyTextEvent(out, event) {
  if (event.type === 'response.output_text.delta') appendOutputText(out, event.delta)
  if (event.type === 'response.output_text.done') replaceOutputText(out, event.text)
  if (event.type === 'response.refusal.delta') appendOutputText(out, event.delta)
  if (event.type === 'response.refusal.done') replaceOutputText(out, event.refusal)
  if (event.type === 'response.content_part.done') applyContentPart(out, event.part || {})
}

function applyImageEvent(out, event) {
  if (event.type === 'response.image_generation_call.partial_image') applyPartialImage(out, event)
  if (event.type === 'response.output_item.done') applyOutputItem(out, event.item || {})
  if (event.type === 'response.completed') applyCompletedResponse(out, event.response || {})
}

function applyCompletedResponse(out, response) {
  if (response.usage) out.usage = response.usage
  if (response.error) applyError(out, response.error)
  if (response.incomplete_details) applyError(out, response.incomplete_details)
  if (!Array.isArray(response.output)) return
  for (const item of response.output) applyOutputItem(out, item)
}

function applyOutputItem(out, item) {
  applyImageCall(out, item)
  if (!item) return
  if (item.error) applyError(out, item.error)
  if (Array.isArray(item.content)) {
    for (const part of item.content) applyContentPart(out, part)
  }
}

function applyContentPart(out, part) {
  if (!part) return
  const text = part.text || part.refusal || ''
  if (text) replaceOutputText(out, text)
}

function applyImageCall(out, item) {
  if (!item || item.type !== 'image_generation_call') return
  if (item.result) out.image = item.result
  if (item.revised_prompt) out.revised_prompt = item.revised_prompt
}

function applyPartialImage(out, event) {
  const b64 = String(event.partial_image_b64 || '').trim()
  if (!b64) return
  out.partial_images.push({
    partial_image_index: optionalNumber(event.partial_image_index, 0, 1000) || 0,
    b64_json: b64,
    output_format: optionalString(event.output_format),
  })
}

function applyError(out, error) {
  if (!error) return
  const message = upstreamErrorText(error)
  if (!message) return
  const code = compactUpstreamText(error.code || error.type || '')
  const stableCode = stableProviderErrorCode(code, message)
  out.error_code = stableCode
  out.error = stableCode && stableCode !== message ? stableCode + ': ' + message : message
}

// 只根据上游明确返回的错误码或拒绝文本分类，绝不能读取生成 Prompt 猜测安全原因。
function stableProviderErrorCode(code, message) {
  const value = compactUpstreamText(code + ' ' + message).toLowerCase()
  if (
    value.includes('adult_sensual_policy_blocked') ||
    value.includes('adult sensual content') ||
    value.includes('adult sensual policy')
  ) {
    return 'adult_sensual_policy_blocked'
  }
  if (
    value.includes('safety_violations') ||
    value.includes('safety system') ||
    value.includes('content policy') ||
    value.includes('policy violation') ||
    value.includes('sexual content') ||
    value.includes('sexually explicit')
  ) {
    return 'unsafe_other'
  }
  if (
    value.includes('temporarily unavailable') ||
    value.includes('try again later') ||
    value.includes('timeout') ||
    value.includes('connection reset')
  ) {
    return 'temporary_unavailable'
  }
  if (
    value.includes('image generation is unavailable for this request') ||
    value.includes('image generation is not available for this request')
  ) {
    return 'invalid_request'
  }
  return compactUpstreamText(code)
}

function resultError(result) {
  if (result.error) return result.error
  return missingImageError(result.types || [])
}

function appendOutputText(out, value) {
  out.output_text = (out.output_text + String(value || '')).slice(0, MAX_UPSTREAM_ERROR_LENGTH)
}

function replaceOutputText(out, value) {
  const text = compactUpstreamText(value)
  if (text) out.output_text = text
}

function upstreamErrorText(error) {
  if (typeof error === 'string') return compactUpstreamText(error)
  const direct = firstNonEmpty([error.message, error.error, error.detail, error.reason, error.code])
  if (direct) return compactUpstreamText(direct)
  try {
    return compactUpstreamText(JSON.stringify(error))
  } catch (_) {
    return ''
  }
}

function compactUpstreamText(value) {
  const text = String(value || '')
    .replace(/\s+/g, ' ')
    .trim()
  return text.length > MAX_UPSTREAM_ERROR_LENGTH
    ? text.slice(0, MAX_UPSTREAM_ERROR_LENGTH) + '…'
    : text
}

function missingImageError(types) {
  const counts = {}
  for (const type of types) counts[type] = (counts[type] || 0) + 1
  const summary = unique(types)
    .map((type) => type + (counts[type] > 1 ? '×' + counts[type] : ''))
    .join(', ')
  return 'No image_generation_call.result found.' + (summary ? ' events: ' + summary : '')
}

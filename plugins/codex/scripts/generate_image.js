const ENDPOINT = 'https://chatgpt.com/backend-api/codex/responses'
const MODEL = 'gpt-5.5'
const INSTRUCTIONS =
  "Use the image_generation tool to create the requested image. Do not rewrite, expand, polish, or add creative details to the user's prompt. Preserve the user intent, exact text, subject, style, composition, and constraints. Return the image generation result."

function main(ctx) {
  let auth = codexEnsureFreshAuth(ctx)
  const request = buildRequest(ctx)
  const images = []
  const partialImages = []

  for (let index = 0; index < request.count; index++) {
    const prompt = promptForIndex(request.prompt, index, request.count)
    const payload = buildPayload(ctx, prompt, request)
    const response = requestImageWithAuthRetry(ctx, auth, payload)
    auth = response.auth
    const result = response.result
    const dimensions = dimensionsFromPng(result.image)
    appendPartialImages(partialImages, result.partial_images, index, request.output_format)
    images.push({
      url: 'data:' + mimeForOutputFormat(request.output_format) + ';base64,' + result.image,
      width: dimensions.width,
      height: dimensions.height,
      revised_prompt: result.revised_prompt || null,
      usage: result.usage || null,
    })
  }

  return {
    success: true,
    status: 'done',
    source: 'codex',
    imageCount: images.length,
    partial_images: partialImages,
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
    quality: normalizeString(ctx.quality, 'auto'),
    background: normalizeString(ctx.background, 'auto'),
    output_format: normalizeString(ctx.output_format, 'png'),
    output_compression: optionalNumber(ctx.output_compression, 0, 100),
    partial_images: optionalNumber(ctx.partial_images, 0, 3),
    moderation: optionalString(ctx.moderation),
    input_fidelity: optionalString(ctx.input_fidelity),
    action: resolveAction(ctx),
    mask_image_url: resolveMaskImageUrl(ctx),
  }
}

function promptForIndex(prompt, index, count) {
  return count > 1 ? prompt + '\n\nCreate variation ' + (index + 1) + ' of ' + count + '.' : prompt
}

function buildPayload(ctx, prompt, request) {
  const content = []
  appendReferenceImages(content, ctx)
  content.push({ type: 'input_text', text: prompt || '' })

  return {
    model: MODEL,
    instructions: INSTRUCTIONS,
    input: [{ role: 'user', content }],
    reasoning: { effort: 'low' },
    tools: [buildImageTool(request)],
    tool_choice: { type: 'image_generation' },
    store: false,
    stream: true,
  }
}

function appendReferenceImages(content, ctx) {
  for (const imageUrl of referenceImageUrls(ctx)) {
    content.push({
      type: 'input_text',
      text: 'The next image is a reference image. Use it only for identity, style, composition, or visual guidance.',
    })
    content.push({ type: 'input_image', image_url: imageUrl })
  }
}

function referenceImageUrls(ctx) {
  const localUrls = localReferenceDataUrls(ctx)
  if (localUrls.length) return localUrls
  const dataUrls = arrayOfStrings(ctx.ref_image_dataurls)
  if (dataUrls.length) return dataUrls
  return arrayOfStrings(ctx.ref_image_urls)
}

function localReferenceDataUrls(ctx) {
  const urls = []
  for (const path of arrayOfStrings(ctx.ref_image_locals)) {
    const file = host.call('file.read_base64', {
      path,
      max_bytes: 50 * 1024 * 1024,
    })
    urls.push('data:' + file.mime + ';base64,' + file.base64)
  }
  return urls
}

function buildImageTool(request) {
  const tool = {
    type: 'image_generation',
    size: request.size,
    quality: request.quality,
    background: request.background,
    output_format: request.output_format,
    action: request.action,
  }
  setOptional(tool, 'output_compression', request.output_compression)
  setOptional(tool, 'partial_images', request.partial_images)
  setOptional(tool, 'moderation', request.moderation)
  setOptional(tool, 'input_fidelity', request.input_fidelity)
  if (request.mask_image_url) {
    tool.input_image_mask = { image_url: request.mask_image_url }
  }
  return tool
}

function requestImageWithAuthRetry(ctx, auth, payload) {
  try {
    return {
      auth,
      result: requestImageWithRetry(codexAccessToken(auth), codexAccountId(auth), payload),
    }
  } catch (error) {
    if (!codexErrorNeedsTokenRefresh(error)) throw error
    const refreshed = codexRefreshAuth(ctx, auth, '生成接口鉴权失败')
    return {
      auth: refreshed,
      result: requestImageWithRetry(
        codexAccessToken(refreshed),
        codexAccountId(refreshed),
        payload
      ),
    }
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
  const out = {
    image: '',
    revised_prompt: '',
    usage: null,
    error: '',
    error_code: '',
    types: [],
    partial_images: [],
  }
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
  if (event.type === 'response.image_generation_call.partial_image') applyPartialImage(out, event)
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

function resolveSize(ctx) {
  const requestedSize = String(ctx.size || ctx.image_size || '').trim()
  if (requestedSize === 'auto') return 'auto'
  if (/^\d+x\d+$/.test(requestedSize)) return requestedSize
  const ratio = String(ctx.image_aspect_ratio || requestedSize || '9:16')
  return sizeForRatio(ratio)
}

function sizeForRatio(ratio) {
  if (ratio === '16:9' || ratio === '4:3') return '1536x1024'
  if (ratio === '9:16' || ratio === '3:4') return '1024x1536'
  return '1024x1024'
}

function resolveAction(ctx) {
  const action = String(ctx.image_action || '').trim()
  if (action === 'edit' || action === 'generate') return action
  return hasReferences(ctx) ? 'edit' : 'generate'
}

function hasReferences(ctx) {
  return (
    arrayOfStrings(ctx.ref_image_locals).length > 0 ||
    arrayOfStrings(ctx.ref_image_dataurls).length > 0 ||
    arrayOfStrings(ctx.ref_image_urls).length > 0
  )
}

function resolveMaskImageUrl(ctx) {
  return optionalString(ctx.mask_image_dataurl) || optionalString(ctx.mask_image_url)
}

function normalizeString(value, fallback) {
  const text = String(value || '').trim()
  return text || fallback
}

function optionalString(value) {
  const text = String(value || '').trim()
  return text || null
}

function optionalNumber(value, min, max) {
  if (value === null || value === undefined || value === '') return null
  const n = Number(value)
  if (!Number.isFinite(n)) return null
  return Math.max(min, Math.min(max, Math.floor(n)))
}

function setOptional(target, key, value) {
  if (value === null || value === undefined || value === '') return
  target[key] = value
}

function arrayOfStrings(value) {
  if (!Array.isArray(value)) return []
  return value.map((item) => String(item || '').trim()).filter(Boolean)
}

function mimeForOutputFormat(format) {
  if (format === 'jpeg') return 'image/jpeg'
  if (format === 'webp') return 'image/webp'
  return 'image/png'
}

function appendPartialImages(target, partials, imageIndex, fallbackFormat) {
  for (const partial of Array.isArray(partials) ? partials : []) {
    target.push({
      image_index: imageIndex,
      partial_image_index: optionalNumber(partial.partial_image_index, 0, 1000) || 0,
      b64_json: partial.b64_json,
      output_format: partial.output_format || fallbackFormat,
    })
  }
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

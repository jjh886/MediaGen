const ENDPOINT = 'https://chatgpt.com/backend-api/codex/responses'
const MODEL = 'gpt-5.6-luna'
const INSTRUCTIONS =
  "When invoking the image_generation tool, use the user's image prompt verbatim. Do not rewrite, expand, summarize, embellish, translate, normalize punctuation, or add or remove visual details or constraints. Preserve the original language, wording, capitalization, quotes, and punctuation exactly."

function main(ctx) {
  if (ctx.reasoning_effort) throw new Error('invalid_input: 图片模型不支持思考深度参数')
  let auth = codexEnsureFreshAuth(ctx)
  const request = buildRequest(ctx)
  const saved = codexImageCheckpoint()
  const images = Array.isArray(saved.images) ? [...saved.images] : []
  const partialImages = []

  for (let index = images.length; index < request.count; index++) {
    const prompt = promptForIndex(request.prompt, index, request.count)
    const payload = buildPayload(ctx, prompt, request)
    const response = requestImageWithAuthRetry(ctx, auth, payload)
    auth = response.auth
    const result = response.result
    const size = /^(\d+)x(\d+)$/.exec(String(result.size || ''))
    const dimensions = size
      ? { width: Number(size[1]), height: Number(size[2]) }
      : dimensionsFromPng(result.image)
    const outputFormat = result.output_format || request.output_format
    appendPartialImages(partialImages, result.partial_images, index, request.output_format)
    images.push({
      url: 'data:' + mimeForOutputFormat(outputFormat) + ';base64,' + result.image,
      width: dimensions.width,
      height: dimensions.height,
      revised_prompt: result.revised_prompt || null,
      usage: result.usage || null,
      quality: result.quality || null,
      background: result.background || null,
    })
    codexImageCheckpoint({ phase: 'ready', images })
  }

  return {
    success: true,
    status: 'done',
    source: 'codex',
    imageCount: images.length,
    partial_images: partialImages,
    quality: images[0]?.quality,
    background: images[0]?.background,
    usage: codexImageUsage(images),
    images,
  }
}

// 只汇总上游明确提供的图片用量；不能把 Responses 文本 token 伪装成图片用量。
function codexImageUsage(images) {
  const usage = {
    input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
    input_tokens_details: { image_tokens: 0, text_tokens: 0 },
  }
  for (const image of images) {
    const value = image.usage
    for (const key of ['input_tokens', 'output_tokens', 'total_tokens']) {
      if (!Number.isFinite(value?.[key]) || value[key] < 0) return undefined
      usage[key] += value[key]
    }
    for (const key of ['image_tokens', 'text_tokens']) {
      if (
        !Number.isFinite(value?.input_tokens_details?.[key]) ||
        value.input_tokens_details[key] < 0
      )
        return undefined
      usage.input_tokens_details[key] += value.input_tokens_details[key]
    }
  }
  return images.length ? usage : undefined
}

function buildRequest(ctx) {
  const prompt = String(ctx.prompt || '').trim()
  if (!prompt) throw new Error('Missing prompt.')
  return {
    prompt,
    model: codexImageModel(ctx),
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
    reasoning: { effort: 'medium', summary: 'auto' },
    tools: [buildImageTool(request)],
    tool_choice: { type: 'image_generation' },
    parallel_tool_calls: true,
    include: ['reasoning.encrypted_content'],
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
    model: request.model,
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
    const saved = codexImageCheckpoint()
    codexImageCheckpoint({ ...saved, phase: 'ready' })
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

function codexImageCheckpoint(value) {
  return host.checkpoint(value) || {}
}

function requestImageWithRetry(accessToken, accountId, payload) {
  const saved = codexImageCheckpoint()
  if (saved.pending_image?.image && !saved.pending_image.error) return saved.pending_image
  if (saved.phase === 'submitting')
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 原生成提交状态待核对')
  codexImageCheckpoint({ ...saved, phase: 'submitting' })
  const result = requestImage(accessToken, accountId, payload)
  if (result.error || !result.image) throw new Error(resultError(result))
  return result
}

function requestImage(accessToken, accountId, payload) {
  const headers = codexUpstreamHeaders(accessToken, accountId, 'text/event-stream')
  const direct = codexDirectImageRequest(payload)
  if (direct) {
    const native = requestImageStream(headers, direct.url, direct.body)
    if (!codexCanFallbackImageEndpoint(native.sse, native.result)) return native.result
  }
  return requestImageStream(headers, ENDPOINT, payload).result
}

function requestImageStream(headers, url, payload) {
  const result = newImageResult()
  const options = payload.tools?.find((tool) => tool.type === 'image_generation') || payload
  const types = []
  const sse = host.stream(
    'http.sse_request',
    {
      method: 'POST',
      url,
      proxy: 'system',
      fingerprint: 'none',
      headers,
      body: payload,
      timeout_ms: 3 * 60 * 60 * 1000,
      retry: { max_attempts: 1 },
    },
    (event) => {
      collectEventType(types, event)
      applyEvent(result, event)
      if (result.error) {
        codexImageCheckpoint({
          ...codexImageCheckpoint(),
          phase: 'submitting',
          pending_image: null,
        })
      } else if (result.image) {
        const saved = codexImageCheckpoint()
        if (!saved.pending_image?.image)
          codexImageCheckpoint({ ...saved, phase: 'image_ready', pending_image: result })
      }
      if (String(event.type || '').endsWith('.partial_image')) {
        return {
          type: 'image.partial',
          b64_json: event.b64_json || event.partial_image_b64,
          partial_image_index: event.partial_image_index || 0,
          created_at: event.created_at || Math.floor(Date.now() / 1000),
          background: event.background || options.background || 'auto',
          quality: event.quality || options.quality || 'auto',
          size: event.size || options.size || 'auto',
          output_format: event.output_format || options.output_format || 'png',
        }
      }
      return null
    }
  )
  if (sse.ok === false) applyHttpError(result, sse)
  finalizeImageResult(result, types)
  if (result.error) {
    codexImageCheckpoint({ ...codexImageCheckpoint(), phase: 'submitting', pending_image: null })
  } else if (result.image) {
    codexImageCheckpoint({ ...codexImageCheckpoint(), phase: 'image_ready', pending_image: result })
  }
  return { result, sse }
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
  // PNG 尺寸只需读取前 24 字节。解码整张图片会把数 MB Base64
  // 膨胀成巨大的 JS Number 数组，并触发 QuickJS 内存上限。
  const bytes = base64HeaderBytes(imageBase64, 24)
  const isPng = bytes.length >= 24 && bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e
  if (!isPng) return { width: 0, height: 0 }
  return {
    width: readUInt32BE(bytes, 16),
    height: readUInt32BE(bytes, 20),
  }
}

function base64HeaderBytes(text, byteCount) {
  const encodedLength = Math.ceil(byteCount / 3) * 4
  const clean = String(text || '')
    .slice(0, encodedLength)
    .replace(/[^A-Za-z0-9+/=]/g, '')
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

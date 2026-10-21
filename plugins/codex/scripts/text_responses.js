const CODEX_RESPONSES_ENDPOINT = 'https://chatgpt.com/backend-api/codex/responses'
const CODEX_TEXT_DEFAULT_MODEL = 'gpt-6-luna'
const CODEX_UNSUPPORTED_RESPONSE_FIELDS = [
  'chat_template_kwargs',
  'stop_sequences',
  'truncation',
  'frequency_penalty',
  'max_completion_tokens',
  'max_output_tokens',
  'metadata',
  'presence_penalty',
  'prompt_cache_retention',
  'safety_identifier',
  'stream_options',
  'temperature',
  'top_p',
  'user',
]

function codexNormalizeResponsesRequest(value, platformConfig) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('invalid_input: Codex Responses request 必须是对象。')
  }
  const payload = JSON.parse(JSON.stringify(value))
  const options =
    payload.options && typeof payload.options === 'object' && !Array.isArray(payload.options)
      ? payload.options
      : {}
  const requestedModel = String(payload.model || '').trim()
  const aliases = ['codex text', 'codex-text', 'codex-responses']
  payload.model = aliases.includes(requestedModel.toLowerCase())
    ? String(platformConfig?.model || CODEX_TEXT_DEFAULT_MODEL).trim()
    : requestedModel || String(platformConfig?.model || CODEX_TEXT_DEFAULT_MODEL).trim()
  if (!codexSupportedTextModel(payload.model)) {
    throw new Error('unsupported_model: Codex 不支持文本模型 ' + payload.model)
  }
  if (payload.service_tier !== undefined) {
    const tier = String(payload.service_tier).trim().toLowerCase()
    const normalizedTier = tier === 'fast' ? 'priority' : tier
    if (!['auto', 'default', 'priority', 'flex', 'scale', 'ultrafast'].includes(normalizedTier)) {
      throw new Error('invalid_input: Codex 不支持该 service_tier')
    }
    payload.service_tier = normalizedTier
  }
  payload.input = codexNormalizeResponsesInput(payload.input)
  codexPromoteSystemInstructions(payload)
  codexNormalizeFunctionTools(payload)
  codexNormalizeFunctionChoice(payload)
  if (!payload.reasoning && options.reasoning_effort) {
    payload.reasoning = { effort: codexReasoningEffort(options.reasoning_effort) }
  }
  if (typeof options.session_key === 'string' && options.session_key)
    payload.prompt_cache_key = options.session_key
  delete payload.options
  payload.store = false
  payload.stream = true
  for (const field of CODEX_UNSUPPORTED_RESPONSE_FIELDS) delete payload[field]
  codexEnsureReasoningEncryptedContent(payload)
  if (!String(payload.instructions || '').trim()) {
    payload.instructions =
      'Follow the user instructions carefully. Use the supplied functions when they are needed.'
  }
  return { requested_stream: value.stream === true, payload }
}

function codexReasoningEffort(value) {
  return String(value || '')
    .trim()
    .toLowerCase()
}

// 账号目录优先；接口不可用时使用随插件发布的模型能力。
function codexApplyReasoning(ctx, auth, payload) {
  let capability = ctx.model_capabilities?.reasoning
  const response = host.tryCall('http.request', {
    method: 'GET',
    url: 'https://chatgpt.com/backend-api/codex/models?client_version=' + CODEX_UPSTREAM_VERSION,
    headers: codexUpstreamHeaders(
      codexAccessToken(auth),
      codexAccountId(auth),
      'application/json',
      ctx
    ),
    proxy: 'system',
    timeout_ms: 15000,
  })
  const models = codexHttpSucceeded(response) && response.data.body?.models
  const model = Array.isArray(models) && models.find((item) => item.slug === payload.model)
  if (Array.isArray(models) && !model) {
    throw new Error('UNSUPPORTED_MODEL: 当前 Codex 账号不支持模型 ' + payload.model)
  }
  if (model && Array.isArray(model.supported_reasoning_levels)) {
    capability = {
      efforts: model.supported_reasoning_levels.map((item) => item.effort),
      default_effort: model.default_reasoning_level,
    }
  }
  // 模型目录与生成接口档位不完全一致；Responses 接口实测拒绝 ultra。
  if (capability) {
    capability = {
      ...capability,
      efforts: (capability.efforts || []).filter((effort) =>
        codexSupportedEffort(effort, payload.model)
      ),
    }
    if (!capability.efforts.includes(capability.default_effort)) {
      capability.default_effort = undefined
    }
  }
  const requested = codexReasoningEffort(payload.reasoning?.effort)
  if (requested && !capability?.efforts?.includes(requested)) {
    throw new Error('invalid_input: ' + payload.model + ' 不支持思考深度 ' + requested)
  }
  const effort = requested || capability?.default_effort
  if (effort) payload.reasoning = { ...payload.reasoning, effort }
  else if (payload.reasoning) delete payload.reasoning.effort
  codexEnsureReasoningEncryptedContent(payload)
}

function codexNormalizeResponsesInput(value) {
  if (typeof value === 'string') {
    if (!value.trim()) throw new Error('missing_input: Codex Responses request 缺少 input。')
    return [{ type: 'message', role: 'user', content: value }]
  }
  if (!Array.isArray(value) || !value.length) {
    throw new Error('missing_input: Codex Responses request 缺少 input。')
  }
  return value.map((item) => {
    if (!item || typeof item !== 'object' || Array.isArray(item)) {
      throw new Error('invalid_input: Codex Responses input 项必须是对象。')
    }
    const next = { ...item }
    if (!next.type && next.role) next.type = 'message'
    if (next.type === 'message' && next.role === 'tool') {
      return {
        type: 'function_call_output',
        call_id: String(next.tool_call_id || next.call_id || ''),
        output: codexResponsesText(next.content),
      }
    }
    if (next.type === 'message' && Array.isArray(next.content))
      next.content = next.content.map(codexNormalizeTextFile)
    return next
  })
}

function codexNormalizeTextFile(part) {
  if (part?.type !== 'input_file' || typeof part.file_data !== 'string') return part
  const match = part.file_data.match(/^data:([^;,]+);base64,([A-Za-z0-9+/=]+)$/)
  const name = String(part.filename || 'attachment.txt')
  const mime = String(match?.[1] || part.mime_type || '').toLowerCase()
  const textExtension =
    /\.(txt|md|csv|json|py|js|jsx|ts|tsx|go|rs|java|c|cpp|h|cs|php|rb|sh|sql|html|css|xml|yaml|yml|toml|log)$/i
  if (
    !mime.startsWith('text/') &&
    !['application/json', 'application/xml', 'application/javascript'].includes(mime) &&
    !(!mime && textExtension.test(name))
  )
    return part
  const encoded = match ? match[2] : part.file_data
  const bytes = host.call('crypto.base64_decode_latin1', { text: encoded }).value
  let text
  try {
    text = decodeURIComponent(
      Array.from(bytes, (char) => '%' + char.charCodeAt(0).toString(16).padStart(2, '0')).join('')
    )
  } catch (_) {
    throw new Error('invalid_input: Codex 文本附件必须使用有效 UTF-8 编码')
  }
  // 上游直接接收纯文本文件时会丢失文件身份；显式保留文件名和内容，仍作为用户数据。
  return {
    type: 'input_text',
    text: '附件内容（以下 JSON 为文件数据）：\n' + JSON.stringify({ filename: name, text }),
  }
}

function codexPromoteSystemInstructions(payload) {
  const input = []
  for (const item of payload.input) {
    if (item.type === 'message' && item.role === 'system') {
      // 系统消息按上游支持的 developer 角色发送一次；不能同时复制到
      // instructions，否则长任务既重复消耗上下文，也会命中该字段的字符上限。
      input.push({ ...item, role: 'developer' })
    } else {
      input.push(item)
    }
  }
  payload.input = input
}

function codexNormalizeFunctionTools(payload) {
  if (payload.tools === undefined) return
  if (!Array.isArray(payload.tools)) throw new Error('invalid_input: tools 必须是数组。')
  payload.tools = payload.tools.map((tool) => {
    if (!tool || typeof tool !== 'object') throw new Error('invalid_input: tool 必须是对象。')
    if (tool.type !== 'function' || !tool.function) return tool
    const fn = tool.function
    return {
      type: 'function',
      name: fn.name,
      description: fn.description,
      parameters: fn.parameters || {},
      strict: fn.strict === true,
    }
  })
}

function codexNormalizeFunctionChoice(payload) {
  const choice = payload.tool_choice
  if (!choice || typeof choice !== 'object' || choice.type !== 'function') return
  const name = String(choice.name || choice.function?.name || '').trim()
  const tools = Array.isArray(payload.tools) ? payload.tools : []
  if (!tools.some((tool) => tool.type === 'function' && tool.name === name)) {
    throw new Error('invalid_input: tool_choice 指定的工具不存在')
  }
  payload.tool_choice = { type: 'function', name }
}

function codexEnsureReasoningEncryptedContent(payload) {
  if (!payload.reasoning || typeof payload.reasoning !== 'object') return
  const include = Array.isArray(payload.include) ? payload.include : []
  if (!include.includes('reasoning.encrypted_content')) {
    payload.include = [...include, 'reasoning.encrypted_content']
  }
}

function codexExecuteResponses(auth, payload, ctx) {
  const accountId = codexAccountId(auth)
  const headers = codexUpstreamHeaders(codexAccessToken(auth), accountId, 'text/event-stream', ctx)
  let completed = null
  const completedItems = new Map()
  let terminalError = ''
  let streamedText = ''
  const result = host.stream(
    'http.sse_request',
    {
      method: 'POST',
      url: CODEX_RESPONSES_ENDPOINT,
      proxy: 'system',
      fingerprint: 'none',
      headers,
      body: payload,
      retry: { max_attempts: 1 },
      timeout_ms: 3 * 60 * 60 * 1000,
    },
    (event) => {
      const type = String(event?.type || '')
      if (
        type === 'response.output_item.done' &&
        event.item &&
        Number.isInteger(event.output_index)
      ) {
        completedItems.set(event.output_index, event.item)
      }
      if (type === 'response.output_text.delta') streamedText += String(event.delta || '')
      if (type === 'response.output_text.done' && String(event.text || '').trim()) {
        streamedText = String(event.text)
      }
      if (type === 'response.completed' || type === 'response.incomplete') {
        completed = event.response || null
        if (completed && type === 'response.incomplete') completed.status = 'incomplete'
        // Codex 完成事件可能不重复携带输出，使用此前明确完成的输出项。
        if (completed && (!Array.isArray(completed.output) || !completed.output.length)) {
          completed.output = [...completedItems.entries()]
            .sort(([left], [right]) => left - right)
            .map(([, item]) => item)
        }
      }
      if (type === 'response.failed') {
        terminalError = codexResponsesError(event.response) || 'Codex 响应状态为 ' + type
      }
      if (type === 'error')
        terminalError = String(event.message || event.error?.message || 'Codex 流返回错误')
      return event
    }
  )
  const status = Number(result?.status || 0)
  if (!result?.ok && !completed && !terminalError) {
    terminalError = codexResponsesHttpError(result)
  }
  if (completed && typeof completed === 'object') {
    const snapshotText = codexResponsesOutputText(completed)
    completed.output_text = snapshotText || streamedText
  }
  const contextRejected =
    /context_length_exceeded|input exceeds the context window|maximum context length|Invalid ['"]instructions['"]: string too long/i.test(
      terminalError
    )
  if (contextRejected) terminalError = 'invalid_input: context_length_exceeded: ' + terminalError
  return { response: completed, error: terminalError, status, contextRejected }
}

function codexResponsesOutputText(response) {
  if (!response || typeof response !== 'object') return ''
  if (typeof response.output_text === 'string') return response.output_text
  if (!Array.isArray(response.output)) return ''
  const chunks = []
  for (const item of response.output) {
    if (!item || typeof item !== 'object' || !Array.isArray(item.content)) continue
    for (const part of item.content) {
      if (!part || typeof part !== 'object') continue
      if ((part.type === 'output_text' || part.type === 'text') && typeof part.text === 'string') {
        chunks.push(part.text)
      }
    }
  }
  return chunks.join('\n')
}

function codexResponsesError(response) {
  if (!response || typeof response !== 'object') return ''
  return String(response.error?.message || response.incomplete_details?.reason || '')
}

function codexResponsesHttpError(result) {
  const body = result?.body
  const detail =
    body && typeof body === 'object'
      ? body.error?.message || body.message || body.detail || JSON.stringify(body)
      : result?.body_text
  const status = Number(result?.status || 0)
  const code =
    body && typeof body === 'object'
      ? String(body.error?.code || body.error?.type || body.code || '')
      : ''
  const message = String(detail || 'Codex Responses 请求失败')
  const cooldownSeconds = codexResponsesCooldownSeconds(result)
  const cooldownSuffix = cooldownSeconds ? '; retry after ' + cooldownSeconds + ' seconds' : ''
  if (codexResponsesQuotaLikeError(code + ' ' + message)) {
    return 'quota_exhausted: ' + (status ? 'HTTP ' + status + ': ' : '') + message + cooldownSuffix
  }
  if (status === 429) {
    return 'rate_limit_exceeded: HTTP 429: ' + message + cooldownSuffix
  }
  const prefix = status === 400 || status === 422 ? 'invalid_input: ' : ''
  return prefix + (status ? 'HTTP ' + status + ': ' : '') + message
}

function codexResponsesCooldownSeconds(result) {
  return (
    codexResponsesSecondsFromValue(result?.body, 'explicit') ||
    codexResponsesSecondsFromText(result?.body_text) ||
    codexResponsesSecondsFromValue(result?.headers, 'headers') ||
    codexRetryAfterSeconds(result?.headers)
  )
}

function codexResponsesSecondsFromValue(value, mode) {
  const seconds = []
  const visit = (item, key, depth) => {
    if (depth > 20 || item === null || item === undefined) return
    const normalizedKey = String(key || '')
      .trim()
      .toLowerCase()
      .replace(/[-\s]+/g, '_')
    if (
      normalizedKey === 'cooldown_seconds' ||
      normalizedKey === 'retry_after_seconds' ||
      normalizedKey === 'reset_after_seconds' ||
      normalizedKey === 'rate_limit_reset_after_seconds' ||
      normalizedKey.endsWith('_reset_after_seconds')
    ) {
      const value = codexPositiveSeconds(item)
      if (value) seconds.push(value)
    }
    if (
      ['reset_at', 'resets_at', 'rate_limit_reset_at', 'cooldown_until'].includes(normalizedKey)
    ) {
      const value = codexSecondsUntil(item)
      if (value) seconds.push(value)
    }
    if (mode !== 'headers' && typeof item === 'string') {
      const value = codexResponsesSecondsFromText(item)
      if (value) seconds.push(value)
    }
    if (Array.isArray(item)) {
      for (const child of item) visit(child, '', depth + 1)
    } else if (item && typeof item === 'object') {
      for (const [childKey, childValue] of Object.entries(item))
        visit(childValue, childKey, depth + 1)
    }
  }
  visit(value, '', 0)
  return seconds.length ? Math.max(...seconds) : 0
}

function codexResponsesSecondsFromText(text) {
  const units = {
    second: 1,
    seconds: 1,
    sec: 1,
    secs: 1,
    minute: 60,
    minutes: 60,
    min: 60,
    mins: 60,
    hour: 3600,
    hours: 3600,
    hr: 3600,
    hrs: 3600,
    day: 86400,
    days: 86400,
  }
  const pattern = /(\d+)\s*(seconds?|secs?|minutes?|mins?|hours?|hrs?|days?)/gi
  let total = 0
  for (const match of String(text || '')
    .toLowerCase()
    .matchAll(pattern)) {
    total += Number(match[1] || 0) * Number(units[match[2]] || 0)
  }
  return total > 0 ? Math.ceil(total) : 0
}

function codexRetryAfterSeconds(headers) {
  const raw = codexHeaderValue(headers, 'retry-after')
  if (!raw) return 0
  const seconds = codexPositiveSeconds(raw)
  if (seconds) return seconds
  return codexSecondsUntil(raw)
}

function codexHeaderValue(headers, name) {
  const expected = String(name || '').toLowerCase()
  for (const [key, value] of Object.entries(headers || {})) {
    if (String(key).toLowerCase() === expected) return String(value || '').trim()
  }
  return ''
}

function codexPositiveSeconds(value) {
  const seconds = Number(value)
  return Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : 0
}

function codexSecondsUntil(value) {
  const raw = String(value || '').trim()
  const numeric = Number(raw)
  let timestamp = 0
  if (Number.isFinite(numeric) && numeric > 0) {
    timestamp = numeric > 10000000000 ? numeric : numeric * 1000
  } else {
    timestamp = Date.parse(raw)
  }
  const seconds = (timestamp - Date.now()) / 1000
  return Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : 0
}

function codexResponsesQuotaLikeError(text) {
  return /quota|insufficient[_\s-]?quota|usage limit|session\s*budget|sessionbudgetexceeded|limit resets in|free plan limit|limit exceeded/i.test(
    String(text || '')
  )
}

function codexResponsesText(value) {
  if (typeof value === 'string') return value
  if (!Array.isArray(value)) return value == null ? '' : JSON.stringify(value)
  return value
    .map((part) => {
      if (typeof part === 'string') return part
      if (!part || typeof part !== 'object') return ''
      return typeof part.text === 'string' ? part.text : ''
    })
    .filter(Boolean)
    .join('\n')
}

function grokTextCapabilities(id, catalog = {}) {
  const efforts = {
    'grok-4.7': ['low', 'medium', 'high', 'xhigh'],
    'grok-4.3': ['none', 'low', 'medium', 'high'],
    'grok-4.20-0309-reasoning': ['low', 'medium', 'high'],
    'grok-4.20-0309-non-reasoning': ['none'],
    'grok-4.20-multi-agent-0309': ['low', 'medium', 'high', 'xhigh'],
  }
  const menu = Array.isArray(catalog.reasoning_efforts) ? catalog.reasoning_efforts : []
  const allowed = ['none', 'low', 'medium', 'high', 'xhigh', 'max']
  const discovered = [
    ...new Set(
      menu
        .map((item) =>
          String(item?.value || item?.id || '')
            .trim()
            .toLowerCase()
        )
        .filter((effort) => allowed.includes(effort))
    ),
  ]
  const supported = discovered.length
    ? discovered
    : catalog.supports_reasoning_effort === false
      ? ['none']
      : efforts[id] || []
  const preferred = menu.find((item) => item?.default === true)
  const defaultEffort = String(
    preferred?.value ||
      preferred?.id ||
      catalog.reasoning_effort ||
      (id === 'grok-4.7' ? 'high' : '')
  )
    .trim()
    .toLowerCase()
  const positive = (value) => (Number.isSafeInteger(value) && value > 0 ? value : undefined)
  return {
    input_schema: {
      type: 'object',
      required: ['model', 'input'],
      properties: {
        model: { type: 'string' },
        input: { oneOf: [{ type: 'string' }, { type: 'array' }] },
        instructions: { type: 'string' },
        stream: { type: 'boolean' },
        tools: { type: 'array' },
        tool_choice: { oneOf: [{ type: 'string' }, { type: 'object' }] },
        text: { type: 'object' },
        reasoning: { type: 'object' },
        max_output_tokens: { type: 'integer', minimum: 1 },
        temperature: { type: 'number' },
        top_p: { type: 'number' },
      },
    },
    input_modalities: ['text', 'image', 'file'],
    output_modalities: ['text'],
    endpoints: ['/v1/responses', '/v1/chat/completions'],
    streaming: true,
    tool_calling: true,
    transports: ['http'],
    reasoning: {
      efforts: supported,
      default_effort: supported.includes(defaultEffort) ? defaultEffort : undefined,
    },
    context_window: positive(catalog.context_window) || (id === 'grok-4.7' ? 500000 : undefined),
    max_output_tokens:
      positive(catalog.max_completion_tokens) || (id === 'grok-4.7' ? 1000000 : undefined),
    supports_backend_search:
      typeof catalog.supports_backend_search === 'boolean'
        ? catalog.supports_backend_search
        : id === 'grok-4.7'
          ? true
          : undefined,
    unsupported_parameters: [
      'audio',
      'modalities',
      'prediction',
      'stop',
      'logit_bias',
      'seed',
      'frequency_penalty',
      'presence_penalty',
      'logprobs',
    ],
  }
}

function grokText(ctx, auth, request, publish = true) {
  const body = JSON.parse(JSON.stringify(request))
  if (Array.isArray(body.input)) body.input = body.input.map(grokInputItem)
  // metadata 由公共 Responses 存储层保存，上游 CLI 接口不接受此字段。
  delete body.metadata
  delete body.options
  delete body.client_metadata
  delete body.stream_options
  body.stream = true
  body.store = false
  body.include = [...new Set([...(body.include || []), 'reasoning.encrypted_content'])]
  for (const item of Array.isArray(body.input) ? body.input : []) {
    if (item.type === 'reasoning') {
      for (const part of item.content || []) if (!part.type) part.type = 'reasoning_text'
    }
  }
  let response = null
  let failed = false
  const items = new Map()
  host.checkpoint({ phase: 'submitting' })
  const result = host.stream(
    'http.sse_request',
    {
      method: 'POST',
      url: GROK_BUILD + '/responses',
      fingerprint: 'none',
      headers: { ...grokHeaders(ctx, auth, true), accept: 'text/event-stream' },
      body,
      retry: { max_attempts: 1 },
      timeout_ms: 600000,
    },
    (event) => {
      if (event.type === 'response.output_item.done' && Number.isInteger(event.output_index))
        items.set(event.output_index, event.item)
      if (['response.completed', 'response.incomplete'].includes(event.type)) {
        response = event.response
        if (response && !response.output?.length)
          response.output = [...items.entries()].sort(([a], [b]) => a - b).map(([, item]) => item)
      }
      if (['error', 'response.failed'].includes(event.type)) failed = true
      return publish ? event : null
    }
  )
  if (!result.ok) {
    if (result.status >= 400 && result.status < 500 && result.status !== 408)
      host.checkpoint({ phase: 'ready' })
    grokError(result)
  }
  if (failed || !response) throw new Error('Grok 文本响应未正常完成')
  return response
}

// Build 对历史 assistant 消息的字段较严格；保留内容并去掉仅属于输出对象的标识。
function grokInputItem(item) {
  if (!item || typeof item !== 'object') throw new Error('invalid_input: input 项必须是对象')
  if (item.type && item.type !== 'message') return item
  const role = item.role || 'user'
  let content = item.content
  if (Array.isArray(content)) {
    if (
      role === 'assistant' &&
      content.every((part) => ['text', 'input_text', 'output_text', 'refusal'].includes(part.type))
    ) {
      content = content.map((part) => part.text || part.refusal || '').join('\n')
    } else {
      content = content.map((part) => {
        if (['text', 'input_text', 'output_text', 'refusal'].includes(part.type))
          return {
            type: role === 'assistant' ? 'output_text' : 'input_text',
            text: part.text || part.refusal || '',
          }
        if (part.type === 'input_image') {
          const detail = part.detail || 'auto'
          if (!['auto', 'low', 'high'].includes(detail))
            throw new Error('invalid_input: Grok 图片 detail 仅支持 auto、low、high')
          return { type: 'input_image', image_url: part.image_url, file_id: part.file_id, detail }
        }
        if (part.type === 'input_file')
          return {
            type: 'input_file',
            file_data: part.file_data,
            file_url: part.file_url,
            file_id: part.file_id,
            filename: part.filename,
          }
        throw new Error('invalid_input: Grok 不支持此消息内容类型')
      })
    }
  }
  if (typeof content !== 'string' && !Array.isArray(content))
    throw new Error('invalid_input: 消息 content 必须是文本或数组')
  return { type: 'message', role, content }
}

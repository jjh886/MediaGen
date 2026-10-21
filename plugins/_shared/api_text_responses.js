function apiTextNormalizeTextRequest(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('invalid_input: 文本模型 request 必须是对象。')
  }
  const options = value.options === undefined ? {} : value.options
  if (!options || typeof options !== 'object' || Array.isArray(options)) {
    throw new Error('invalid_input: 文本模型 request.options 必须是对象。')
  }
  if (
    options.platform_session != null &&
    (!options.platform_session ||
      typeof options.platform_session !== 'object' ||
      Array.isArray(options.platform_session))
  )
    throw new Error('invalid_input: 平台会话信息必须是对象')
  const input = apiTextNormalizeResponsesInput(value.input)
  if (!input.length) throw new Error('missing_input: 文本模型 request 缺少 input。')
  const maxOutputTokens = Number(value.max_output_tokens ?? options.max_output_tokens ?? 0)
  if (!Number.isInteger(maxOutputTokens) || maxOutputTokens < 0) {
    throw new Error('invalid_input: max_output_tokens 必须是非负整数。')
  }
  const reasoningEffort = apiTextNormalizeReasoningEffort(
    value.reasoning?.effort ?? options.reasoning_effort
  )
  const tools = apiTextNormalizeResponsesTools(value.tools)
  const toolChoice = apiTextNormalizeToolChoice(value.tool_choice, tools)
  if (value.instructions !== undefined && typeof value.instructions !== 'string') {
    throw new Error('invalid_input: instructions 必须是字符串。')
  }
  if (value.stream !== undefined && typeof value.stream !== 'boolean') {
    throw new Error('invalid_input: stream 必须是布尔值。')
  }
  if (value.reasoning !== undefined && (!value.reasoning || typeof value.reasoning !== 'object')) {
    throw new Error('invalid_input: reasoning 必须是对象。')
  }
  if (value.text !== undefined && (!value.text || typeof value.text !== 'object')) {
    throw new Error('invalid_input: text 必须是对象。')
  }
  if (reasoningEffort && !/^[a-z][a-z0-9_-]{0,31}$/.test(reasoningEffort)) {
    throw new Error('invalid_input: reasoning_effort 不受支持。')
  }
  return {
    session:
      options.platform_session &&
      typeof options.platform_session === 'object' &&
      !Array.isArray(options.platform_session)
        ? options.platform_session
        : null,
    session_key:
      typeof options.session_key === 'string'
        ? options.session_key
        : typeof value.prompt_cache_key === 'string'
          ? value.prompt_cache_key
          : '',
    ...(Number.isSafeInteger(options.session_input_start)
      ? { session_input_start: options.session_input_start }
      : {}),
    model: String(value.model || '').trim(),
    input,
    instructions: String(value.instructions || '').trim(),
    max_output_tokens: maxOutputTokens,
    reasoning: { ...(value.reasoning || {}), effort: reasoningEffort },
    tools,
    tool_choice: toolChoice,
    parallel_tool_calls: value.parallel_tool_calls !== false,
    text: value.text || null,
    stream: value.stream === true,
  }
}

function apiTextNormalizeReasoningEffort(value) {
  return String(value || '')
    .trim()
    .toLowerCase()
}

const API_TEXT_TOOL_OPEN = '<tool_call>'
const API_TEXT_TOOL_CLOSE = '</tool_call>'

function apiTextNormalizeResponsesInput(value) {
  const values = typeof value === 'string' ? [value] : value
  if (!Array.isArray(values)) throw new Error('invalid_input: input 必须是字符串或数组。')
  return values
    .map(apiTextNormalizeResponsesItem)
    .filter((item) => item.text || item.attachments?.length)
}

function apiTextNormalizeResponsesItem(value) {
  if (typeof value === 'string') return { role: 'user', text: value.trim() }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('invalid_input: input 项必须是字符串或对象。')
  }
  const type = String(value.type || 'message')
  if (type === 'function_call') {
    const name = String(value.name || '').trim()
    const args =
      typeof value.arguments === 'string' ? value.arguments : JSON.stringify(value.arguments || {})
    return {
      role: 'assistant',
      text:
        API_TEXT_TOOL_OPEN +
        JSON.stringify({ name, arguments: apiTextParseJsonObject(args) }) +
        API_TEXT_TOOL_CLOSE,
    }
  }
  if (type === 'function_call_output') {
    return {
      role: 'tool',
      text:
        '[Tool result ' +
        String(value.call_id || '') +
        ']\n' +
        apiTextResponsesContentText(value.output),
    }
  }
  if (type !== 'message') throw new Error('invalid_input: 不支持的 Responses input 类型：' + type)
  const role = String(value.role || 'user')
    .trim()
    .toLowerCase()
  if (!['user', 'assistant', 'system', 'developer', 'tool'].includes(role)) {
    throw new Error('invalid_input: 不支持的消息角色：' + role)
  }
  const text = apiTextResponsesContentText(value.content).trim()
  const attachments = apiTextResponsesMedia(value.content)
  return attachments.length ? { role, text, attachments } : { role, text }
}

function apiTextResponsesContentText(value) {
  if (typeof value === 'string') return value
  if (!Array.isArray(value)) return value == null ? '' : JSON.stringify(value)
  return value
    .map((part) => {
      if (typeof part === 'string') return part
      if (!part || typeof part !== 'object') return ''
      if (typeof part.text === 'string') return part.text
      if (typeof part.refusal === 'string') return part.refusal
      return ''
    })
    .filter(Boolean)
    .join('\n')
}

function apiTextNormalizeResponsesTools(value) {
  if (value === undefined) return []
  if (!Array.isArray(value)) throw new Error('invalid_input: tools 必须是数组。')
  return value.map((tool) => {
    if (!tool || typeof tool !== 'object' || tool.type !== 'function') {
      throw new Error('invalid_input: 本地文本仅支持 function 工具。')
    }
    const source = tool.function && typeof tool.function === 'object' ? tool.function : tool
    const name = String(source.name || '').trim()
    if (!name) throw new Error('invalid_input: function 工具缺少 name。')
    return {
      type: 'function',
      name,
      description: String(source.description || ''),
      parameters:
        source.parameters && typeof source.parameters === 'object' ? source.parameters : {},
      strict: source.strict === true,
    }
  })
}

function apiTextNormalizeToolChoice(value, tools) {
  if (value === undefined || value === null) return tools.length ? 'auto' : 'none'
  if (['auto', 'none', 'required'].includes(value)) return value
  if (!value || typeof value !== 'object' || value.type !== 'function') {
    throw new Error('invalid_input: tool_choice 格式无效。')
  }
  const name = String(value.name || value.function?.name || '').trim()
  if (!tools.some((tool) => tool.name === name)) {
    throw new Error('invalid_input: tool_choice 指向未声明的 function。')
  }
  return { type: 'function', name }
}

function apiTextBuildResponsesPrompt(request) {
  const sections = []
  if (request.tools.length) sections.push(apiTextToolInstructions(request))
  if (request.instructions) sections.push('# INSTRUCTIONS\n' + request.instructions)
  const format = request.text?.format
  if (format?.type === 'json_object') sections.push('Return only one valid JSON object.')
  if (format?.type === 'json_schema') {
    sections.push('Return only JSON matching this schema:\n' + JSON.stringify(format.schema || {}))
  }
  sections.push(
    request.input.map((item) => '[' + item.role.toUpperCase() + ']\n' + item.text).join('\n\n')
  )
  return sections.filter(Boolean).join('\n\n')
}

function apiTextToolInstructions(request) {
  const forced = typeof request.tool_choice === 'object' ? request.tool_choice.name : ''
  return [
    '# TOOLS AVAILABLE',
    JSON.stringify(request.tools),
    '# TOOL CALLING FORMAT',
    'To call a tool, output exactly <tool_call>{"name":"tool_name","arguments":{}}</tool_call>.',
    'Use only listed names and valid JSON. Do not write text after tool calls.',
    request.tool_choice === 'none' ? 'Do not call tools.' : '',
    request.tool_choice === 'required' ? 'You must call at least one tool.' : '',
    forced ? 'You must call only the tool named ' + forced + '.' : '',
  ]
    .filter(Boolean)
    .join('\n')
}

function apiTextCreateResponseWriter(request) {
  const responseId = 'resp_' + apiTextUuid().replace(/-/g, '')
  return {
    request,
    response_id: responseId,
    created_at: Math.floor(Date.now() / 1000),
    sequence: 0,
    output: [],
    message: null,
    text: '',
    parser: { buffer: '', inside: false },
  }
}

function apiTextStartResponse(writer) {
  apiTextEmitResponseEvent(writer, 'response.created', {
    response: apiTextResponseObject(writer, 'in_progress'),
  })
  apiTextEmitResponseEvent(writer, 'response.in_progress', {
    response: apiTextResponseObject(writer, 'in_progress'),
  })
}

function apiTextFeedResponseText(writer, delta) {
  if (!delta) return
  writer.parser.buffer += delta
  apiTextDrainToolParser(writer, false)
}

function apiTextDrainToolParser(writer, final) {
  const parser = writer.parser
  while (parser.buffer) {
    if (!parser.inside) {
      const start = parser.buffer.indexOf(API_TEXT_TOOL_OPEN)
      if (start >= 0) {
        apiTextEmitTextDelta(writer, parser.buffer.slice(0, start))
        parser.buffer = parser.buffer.slice(start + API_TEXT_TOOL_OPEN.length)
        parser.inside = true
        continue
      }
      const keep = final ? 0 : apiTextPartialTagLength(parser.buffer, API_TEXT_TOOL_OPEN)
      const end = parser.buffer.length - keep
      apiTextEmitTextDelta(writer, parser.buffer.slice(0, end))
      parser.buffer = parser.buffer.slice(end)
      return
    }
    const end = parser.buffer.indexOf(API_TEXT_TOOL_CLOSE)
    if (end < 0) {
      if (!final) return
      const call = apiTextParseToolCall(parser.buffer, writer.request.tools)
      if (call) apiTextEmitToolCall(writer, call)
      else apiTextEmitTextDelta(writer, API_TEXT_TOOL_OPEN + parser.buffer)
      parser.buffer = ''
      parser.inside = false
      return
    }
    const raw = parser.buffer.slice(0, end)
    const call = apiTextParseToolCall(raw, writer.request.tools)
    if (call) apiTextEmitToolCall(writer, call)
    else apiTextEmitTextDelta(writer, API_TEXT_TOOL_OPEN + raw + API_TEXT_TOOL_CLOSE)
    parser.buffer = parser.buffer.slice(end + API_TEXT_TOOL_CLOSE.length)
    parser.inside = false
  }
}

function apiTextEmitTextDelta(writer, delta) {
  if (!delta) return
  if (!writer.message) {
    const item = {
      id: 'msg_' + apiTextUuid().replace(/-/g, ''),
      type: 'message',
      status: 'in_progress',
      role: 'assistant',
      content: [],
    }
    writer.message = item
    writer.output.push(item)
    const outputIndex = writer.output.length - 1
    apiTextEmitResponseEvent(writer, 'response.output_item.added', {
      output_index: outputIndex,
      item,
    })
    apiTextEmitResponseEvent(writer, 'response.content_part.added', {
      output_index: outputIndex,
      item_id: item.id,
      content_index: 0,
      part: { type: 'output_text', text: '', annotations: [], logprobs: [] },
    })
  }
  writer.text += delta
  const outputIndex = writer.output.indexOf(writer.message)
  apiTextEmitResponseEvent(writer, 'response.output_text.delta', {
    output_index: outputIndex,
    item_id: writer.message.id,
    content_index: 0,
    delta,
    logprobs: [],
  })
}

function apiTextEmitToolCall(writer, call) {
  const item = {
    id: 'fc_' + apiTextUuid().replace(/-/g, ''),
    type: 'function_call',
    status: 'in_progress',
    call_id: 'call_' + apiTextUuid().replace(/-/g, ''),
    name: call.name,
    arguments: '',
  }
  writer.output.push(item)
  const outputIndex = writer.output.length - 1
  apiTextEmitResponseEvent(writer, 'response.output_item.added', {
    output_index: outputIndex,
    item,
  })
  item.arguments = JSON.stringify(call.arguments)
  apiTextEmitResponseEvent(writer, 'response.function_call_arguments.delta', {
    output_index: outputIndex,
    item_id: item.id,
    delta: item.arguments,
  })
  apiTextEmitResponseEvent(writer, 'response.function_call_arguments.done', {
    output_index: outputIndex,
    item_id: item.id,
    name: item.name,
    arguments: item.arguments,
  })
  item.status = 'completed'
  apiTextEmitResponseEvent(writer, 'response.output_item.done', {
    output_index: outputIndex,
    item,
  })
}

function apiTextCompleteResponse(writer) {
  apiTextDrainToolParser(writer, true)
  writer.text = apiTextUnwrapWritingArtifact(writer.text)
  if (writer.message) {
    const index = writer.output.indexOf(writer.message)
    const part = { type: 'output_text', text: writer.text, annotations: [], logprobs: [] }
    apiTextEmitResponseEvent(writer, 'response.output_text.done', {
      output_index: index,
      item_id: writer.message.id,
      content_index: 0,
      text: writer.text,
      logprobs: [],
    })
    apiTextEmitResponseEvent(writer, 'response.content_part.done', {
      output_index: index,
      item_id: writer.message.id,
      content_index: 0,
      part,
    })
    writer.message.content = [part]
    writer.message.status = 'completed'
    apiTextEmitResponseEvent(writer, 'response.output_item.done', {
      output_index: index,
      item: writer.message,
    })
  }
  if (!writer.output.length)
    throw new Error('temporary_unavailable: ChatGPT 完成但没有返回文本或工具调用')
  const response = apiTextResponseObject(writer, 'completed')
  apiTextEmitResponseEvent(writer, 'response.completed', { response })
  return response
}

function apiTextUnwrapWritingArtifact(value) {
  const text = String(value || '').trim()
  const match = text.match(/^:::writing\{[^\n]*\}\s*\n([\s\S]*?)\n:::\s*$/)
  return match ? match[1].trim() : text
}

function apiTextFailResponse(writer, error) {
  const message = String(error?.message || error || 'ChatGPT 文本生成失败')
  const response = apiTextResponseObject(writer, 'failed')
  response.error = { code: 'upstream_error', message }
  apiTextEmitResponseEvent(writer, 'response.failed', { response })
}

function apiTextResponseObject(writer, status) {
  const request = writer.request
  return {
    id: writer.response_id,
    object: 'response',
    created_at: writer.created_at,
    status,
    error: null,
    incomplete_details: null,
    instructions: request.instructions || null,
    max_output_tokens: request.max_output_tokens || null,
    model: request.model || 'auto',
    output: writer.output,
    output_text: writer.text,
    parallel_tool_calls: request.parallel_tool_calls,
    previous_response_id: null,
    reasoning: request.reasoning,
    store: false,
    temperature: null,
    text: request.text || { format: { type: 'text' } },
    tool_choice: request.tool_choice,
    tools: request.tools,
    top_p: null,
    truncation: 'disabled',
    usage: null,
    user: null,
    metadata: writer.metadata || {},
  }
}

// 标题属于可选会话元数据；读取失败不能把已经完成的正文变成失败任务。
function apiTextReadConversationTitle(writer, conversationId, read) {
  if (!conversationId) return
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      const value = read()
      if (typeof value !== 'string') return
      const title = Array.from(value.replace(/\s+/gu, ' ').trim()).slice(0, 120).join('')
      if (title && !/^(new chat|new conversation|新对话|新会话|未命名会话)$/i.test(title)) {
        writer.metadata = { conversation_id: String(conversationId), conversation_title: title }
        return
      }
      if (attempt < 2) host.call('time.sleep', { milliseconds: 500 })
    } catch (_) {
      return
    }
  }
}

function apiTextEmitResponseEvent(writer, type, fields) {
  if (writer.silent) return
  host.emit({ type, sequence_number: writer.sequence++, ...(fields || {}) })
}

function apiTextParseToolCall(raw, tools) {
  const value = apiTextParseJsonObject(
    String(raw || '')
      .replace(/^```\w*|```$/g, '')
      .trim()
  )
  const name = String(value.name || value.function?.name || '').trim()
  if (!tools.some((tool) => tool.name === name)) return null
  const args = value.arguments ?? value.function?.arguments ?? {}
  return { name, arguments: typeof args === 'string' ? apiTextParseJsonObject(args) : args }
}

function apiTextParseJsonObject(value) {
  if (value && typeof value === 'object' && !Array.isArray(value)) return value
  try {
    const parsed = JSON.parse(String(value || '{}'))
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {}
  } catch (_) {
    return {}
  }
}

function apiTextPartialTagLength(value, tag) {
  for (let length = Math.min(value.length, tag.length - 1); length > 0; length--) {
    if (value.endsWith(tag.slice(0, length))) return length
  }
  return 0
}

function apiTextUuid() {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (char) => {
    const value = Math.floor(Math.random() * 16)
    return (char === 'x' ? value : (value & 3) | 8).toString(16)
  })
}

// 平台已经保存前文时仅发送本轮新增消息；新建或失败重试仍使用完整历史。
function apiTextSessionRequest(request) {
  let lastUser = -1
  for (let i = 0; i < request.input.length; i++) if (request.input[i].role === 'user') lastUser = i
  if (lastUser < 0) throw new Error('invalid_input: 继续对话缺少新的用户消息')
  const start =
    request.session_input_start === undefined
      ? lastUser
      : request.session_input_start >= 0 && request.session_input_start <= lastUser
        ? request.session_input_start
        : 0
  const currentContext = request.input
    .slice(0, start)
    .filter((item) => ['system', 'developer'].includes(item.role))
  return { ...request, input: [...currentContext, ...request.input.slice(start)] }
}

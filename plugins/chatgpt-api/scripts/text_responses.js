const CHATGPT_API_TOOL_OPEN = '<tool_call>'
const CHATGPT_API_TOOL_CLOSE = '</tool_call>'

function chatgptApiNormalizeResponsesInput(value) {
  const values = typeof value === 'string' ? [value] : value
  if (!Array.isArray(values)) throw new Error('invalid_input: input 必须是字符串或数组。')
  return values
    .map(chatgptApiNormalizeResponsesItem)
    .filter((item) => item.text || item.images?.length)
}

function chatgptApiNormalizeResponsesItem(value) {
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
        CHATGPT_API_TOOL_OPEN +
        JSON.stringify({ name, arguments: chatgptApiParseJsonObject(args) }) +
        CHATGPT_API_TOOL_CLOSE,
    }
  }
  if (type === 'function_call_output') {
    return {
      role: 'tool',
      text:
        '[Tool result ' +
        String(value.call_id || '') +
        ']\n' +
        chatgptApiResponsesContentText(value.output),
    }
  }
  if (type !== 'message') throw new Error('invalid_input: 不支持的 Responses input 类型：' + type)
  const role = String(value.role || 'user')
    .trim()
    .toLowerCase()
  if (!['user', 'assistant', 'system', 'developer', 'tool'].includes(role)) {
    throw new Error('invalid_input: 不支持的消息角色：' + role)
  }
  const images = Array.isArray(value.content)
    ? value.content.filter((part) => part?.type === 'input_image').map((part) => part.image_url)
    : []
  if (images.some((image) => typeof image !== 'string'))
    throw new Error('invalid_input: 图片缺少 image_url')
  return { role, text: chatgptApiResponsesContentText(value.content).trim(), images }
}

function chatgptApiResponsesContentText(value) {
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

function chatgptApiNormalizeResponsesTools(value) {
  if (value === undefined) return []
  if (!Array.isArray(value)) throw new Error('invalid_input: tools 必须是数组。')
  return value.map((tool) => {
    if (!tool || typeof tool !== 'object' || tool.type !== 'function') {
      throw new Error('invalid_input: ChatGPT 本地文本仅支持 function 工具。')
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

function chatgptApiNormalizeToolChoice(value, tools) {
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

function chatgptApiBuildResponsesPrompt(request) {
  const sections = []
  if (request.tools.length) sections.push(chatgptApiToolInstructions(request))
  if (request.instructions) sections.push('# INSTRUCTIONS\n' + request.instructions)
  if (request.max_output_tokens > 0) {
    sections.push('Limit the response to at most ' + request.max_output_tokens + ' tokens.')
  }
  const format = request.text?.format
  if (format?.type === 'json_object') sections.push('Return only one valid JSON object.')
  if (format?.type === 'json_schema') {
    sections.push('Return only JSON matching this schema:\n' + JSON.stringify(format.schema || {}))
  }
  sections.push(
    request.input
      .map((item) => '[' + item.role.toUpperCase() + ']\n' + (item.image_labels || '') + item.text)
      .join('\n\n')
  )
  return sections.filter(Boolean).join('\n\n')
}

function chatgptApiToolInstructions(request) {
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

function chatgptApiCreateResponseWriter(request) {
  const responseId = 'resp_' + chatgptApiUuid().replace(/-/g, '')
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

function chatgptApiStartResponse(writer) {
  chatgptApiEmitResponseEvent(writer, 'response.created', {
    response: chatgptApiResponseObject(writer, 'in_progress'),
  })
  chatgptApiEmitResponseEvent(writer, 'response.in_progress', {
    response: chatgptApiResponseObject(writer, 'in_progress'),
  })
}

function chatgptApiFeedResponseText(writer, delta) {
  if (!delta) return
  writer.parser.buffer += delta
  chatgptApiDrainToolParser(writer, false)
}

function chatgptApiDrainToolParser(writer, final) {
  const parser = writer.parser
  while (parser.buffer) {
    if (!parser.inside) {
      const start = parser.buffer.indexOf(CHATGPT_API_TOOL_OPEN)
      if (start >= 0) {
        chatgptApiEmitTextDelta(writer, parser.buffer.slice(0, start))
        parser.buffer = parser.buffer.slice(start + CHATGPT_API_TOOL_OPEN.length)
        parser.inside = true
        continue
      }
      const keep = final ? 0 : chatgptApiPartialTagLength(parser.buffer, CHATGPT_API_TOOL_OPEN)
      const end = parser.buffer.length - keep
      chatgptApiEmitTextDelta(writer, parser.buffer.slice(0, end))
      parser.buffer = parser.buffer.slice(end)
      return
    }
    const end = parser.buffer.indexOf(CHATGPT_API_TOOL_CLOSE)
    if (end < 0) {
      if (!final) return
      const call = chatgptApiParseToolCall(parser.buffer, writer.request.tools)
      if (call) chatgptApiEmitToolCall(writer, call)
      else chatgptApiEmitTextDelta(writer, CHATGPT_API_TOOL_OPEN + parser.buffer)
      parser.buffer = ''
      parser.inside = false
      return
    }
    const raw = parser.buffer.slice(0, end)
    const call = chatgptApiParseToolCall(raw, writer.request.tools)
    if (call) chatgptApiEmitToolCall(writer, call)
    else chatgptApiEmitTextDelta(writer, CHATGPT_API_TOOL_OPEN + raw + CHATGPT_API_TOOL_CLOSE)
    parser.buffer = parser.buffer.slice(end + CHATGPT_API_TOOL_CLOSE.length)
    parser.inside = false
  }
}

function chatgptApiEmitTextDelta(writer, delta) {
  if (!delta) return
  if (!writer.message) {
    const item = {
      id: 'msg_' + chatgptApiUuid().replace(/-/g, ''),
      type: 'message',
      status: 'in_progress',
      role: 'assistant',
      content: [],
    }
    writer.message = item
    writer.output.push(item)
    const outputIndex = writer.output.length - 1
    chatgptApiEmitResponseEvent(writer, 'response.output_item.added', {
      output_index: outputIndex,
      item,
    })
    chatgptApiEmitResponseEvent(writer, 'response.content_part.added', {
      output_index: outputIndex,
      item_id: item.id,
      content_index: 0,
      part: { type: 'output_text', text: '', annotations: [], logprobs: [] },
    })
  }
  writer.text += delta
  const outputIndex = writer.output.indexOf(writer.message)
  chatgptApiEmitResponseEvent(writer, 'response.output_text.delta', {
    output_index: outputIndex,
    item_id: writer.message.id,
    content_index: 0,
    delta,
    logprobs: [],
  })
}

function chatgptApiEmitToolCall(writer, call) {
  const item = {
    id: 'fc_' + chatgptApiUuid().replace(/-/g, ''),
    type: 'function_call',
    status: 'in_progress',
    call_id: 'call_' + chatgptApiUuid().replace(/-/g, ''),
    name: call.name,
    arguments: '',
  }
  writer.output.push(item)
  const outputIndex = writer.output.length - 1
  chatgptApiEmitResponseEvent(writer, 'response.output_item.added', {
    output_index: outputIndex,
    item,
  })
  item.arguments = JSON.stringify(call.arguments)
  chatgptApiEmitResponseEvent(writer, 'response.function_call_arguments.delta', {
    output_index: outputIndex,
    item_id: item.id,
    delta: item.arguments,
  })
  chatgptApiEmitResponseEvent(writer, 'response.function_call_arguments.done', {
    output_index: outputIndex,
    item_id: item.id,
    name: item.name,
    arguments: item.arguments,
  })
  item.status = 'completed'
  chatgptApiEmitResponseEvent(writer, 'response.output_item.done', {
    output_index: outputIndex,
    item,
  })
}

function chatgptApiCompleteResponse(writer) {
  chatgptApiDrainToolParser(writer, true)
  writer.text = chatgptApiUnwrapWritingArtifact(writer.text)
  if (writer.message) {
    const index = writer.output.indexOf(writer.message)
    const part = { type: 'output_text', text: writer.text, annotations: [], logprobs: [] }
    chatgptApiEmitResponseEvent(writer, 'response.output_text.done', {
      output_index: index,
      item_id: writer.message.id,
      content_index: 0,
      text: writer.text,
      logprobs: [],
    })
    chatgptApiEmitResponseEvent(writer, 'response.content_part.done', {
      output_index: index,
      item_id: writer.message.id,
      content_index: 0,
      part,
    })
    writer.message.content = [part]
    writer.message.status = 'completed'
    chatgptApiEmitResponseEvent(writer, 'response.output_item.done', {
      output_index: index,
      item: writer.message,
    })
  }
  if (!writer.output.length)
    throw new Error('temporary_unavailable: ChatGPT 完成但没有返回文本或工具调用')
  const response = chatgptApiResponseObject(writer, 'completed')
  chatgptApiEmitResponseEvent(writer, 'response.completed', { response })
  return response
}

function chatgptApiUnwrapWritingArtifact(value) {
  const text = String(value || '').trim()
  const match = text.match(/^:::writing\{[^\n]*\}\s*\n([\s\S]*?)\n:::\s*$/)
  return match ? match[1].trim() : text
}

function chatgptApiFailResponse(writer, error) {
  const message = String(error?.message || error || 'ChatGPT 文本生成失败')
  const response = chatgptApiResponseObject(writer, 'failed')
  response.error = { code: 'upstream_error', message }
  chatgptApiEmitResponseEvent(writer, 'response.failed', { response })
}

function chatgptApiResponseObject(writer, status) {
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
    metadata: {},
  }
}

function chatgptApiEmitResponseEvent(writer, type, fields) {
  if (writer.silent) return
  host.emit({ type, sequence_number: writer.sequence++, ...(fields || {}) })
}

function chatgptApiParseToolCall(raw, tools) {
  const value = chatgptApiParseJsonObject(
    String(raw || '')
      .replace(/^```\w*|```$/g, '')
      .trim()
  )
  const name = String(value.name || value.function?.name || '').trim()
  if (!tools.some((tool) => tool.name === name)) return null
  const args = value.arguments ?? value.function?.arguments ?? {}
  return { name, arguments: typeof args === 'string' ? chatgptApiParseJsonObject(args) : args }
}

function chatgptApiParseJsonObject(value) {
  if (value && typeof value === 'object' && !Array.isArray(value)) return value
  try {
    const parsed = JSON.parse(String(value || '{}'))
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {}
  } catch (_) {
    return {}
  }
}

function chatgptApiPartialTagLength(value, tag) {
  for (let length = Math.min(value.length, tag.length - 1); length > 0; length--) {
    if (value.endsWith(tag.slice(0, length))) return length
  }
  return 0
}

function chatgptApiUploadTextImages(auth, request) {
  const references = []
  for (const item of request.input) {
    const labels = []
    for (const source of item.images || []) {
      if (references.length >= 10) throw new Error('invalid_input: 每次最多支持 10 张图片')
      const match = source.match(/^data:(image\/[a-zA-Z0-9.+-]+);base64,([a-zA-Z0-9+/=\s]+)$/)
      if (!match)
        throw new Error('invalid_input: ChatGPT 图片输入请使用 base64 data URL 或已上传的 file_id')
      const info = host.call('file.inspect_image', { base64: match[2] })
      if (!['image/png', 'image/jpeg', 'image/webp', 'image/gif'].includes(info.mime_type))
        throw new Error('invalid_input: 不支持的图片格式')
      const reference = {
        ...info,
        base64: match[2],
        name: 'input-' + (references.length + 1) + '.' + chatgptApiImageExtension(info.mime_type),
      }
      references.push(chatgptApiUploadReference(auth, reference))
      labels.push('[附件图片 ' + references.length + ']')
    }
    item.image_labels = labels.length ? labels.join(' ') + '\n' : ''
  }
  return references
}

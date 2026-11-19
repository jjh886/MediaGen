function codexNormalizeResponsesInput(value) {
  if (typeof value === 'string') {
    if (!value.trim()) throw new Error('missing_input: Codex Responses request 缺少 input。')
    return [{ type: 'message', role: 'user', content: value }]
  }
  if (!Array.isArray(value) || !value.length) {
    throw new Error('missing_input: Codex Responses request 缺少 input。')
  }
  const declarations = new Set()
  for (const item of value) {
    if (
      item?.type === 'additional_tools' &&
      typeof item.id === 'string' &&
      /^at_[\da-f]{8}(?:-[\da-f]{4}){3}-[\da-f]{12}$/i.test(item.id) &&
      item.id.startsWith('at_')
    )
      declarations.add(item.id)
  }
  return value.map((item) => {
    if (!item || typeof item !== 'object' || Array.isArray(item)) {
      throw new Error('invalid_input: Codex Responses input 项必须是对象。')
    }
    const next = { ...item }
    if (['item_reference', 'additional_tools'].includes(next.type) && declarations.has(next.id))
      next.id = next.id.replace(/-/g, '')
    if (
      next.type === 'function_call_output' &&
      next.namespace === 'agent' &&
      (next.call_id === undefined || (typeof next.call_id === 'string' && !next.call_id.trim()))
    ) {
      const text = codexExternalContext(next.name, next.output)
      if (text) return { type: 'message', role: 'user', content: [{ type: 'input_text', text }] }
    }
    if (['function', 'namespace', 'additional_tools', 'custom', 'tool_search'].includes(next.type))
      codexCleanToolParameters(next)
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

function codexExternalContext(name, output) {
  if (typeof name !== 'string' || typeof output !== 'string' || !output.trim()) return ''
  if (['browser_info', 'goal_objective', 'untrusted_context'].includes(name))
    return '<external_' + name + '>' + output + '</external_' + name + '>'
  let snapshot = output
  if (name !== 'codex_apps_open_page') {
    const part = /^codex_apps_selected_page_part_([1-9]\d*)_of_([1-9]\d*)$/.exec(name)
    if (!part || !Number.isSafeInteger(Number(part[2])) || Number(part[1]) > Number(part[2]))
      return ''
    const end = output.indexOf('\n')
    if (
      end < 0 ||
      output.slice(0, end).replace(/\r$/, '') !==
        'Companion Page snapshot part ' + part[1] + ' of ' + part[2] + ':'
    )
      return ''
    snapshot = output.slice(end + 1)
    if (!snapshot.trim()) return ''
    if (part[2] !== '1') snapshot = null
  }
  if (snapshot !== null) {
    try {
      const id = JSON.parse(snapshot)?.page_id
      if (id !== null && !(typeof id === 'string' && id.trim())) return ''
    } catch {
      return ''
    }
  }
  return '<external_' + name + '>\n' + output + '\n</external_' + name + '>'
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
    codexCleanToolParameters(tool)
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

function codexCleanToolParameters(tool) {
  if (!tool || typeof tool !== 'object') return
  codexCleanSchemaPatterns(tool.parameters)
  if (tool.function) codexCleanToolParameters(tool.function)
  if (Array.isArray(tool.tools)) tool.tools.forEach(codexCleanToolParameters)
}

function codexUnsupportedSchemaPattern(pattern) {
  if (typeof pattern !== 'string') return false
  let inClass = false
  for (let i = 0; i < pattern.length; i++) {
    const ch = pattern[i]
    if (ch === '\\') {
      const escape = pattern[++i]
      if (!escape || (/[a-z]/i.test(escape) && !'abfnrtvdDsSwWAZBNuUx'.includes(escape)))
        return true
    } else if (ch === '[') inClass = true
    else if (ch === ']') inClass = false
    else if (
      !inClass &&
      ch === '(' &&
      pattern[i + 1] === '?' &&
      ["'", '=', '!', '<'].includes(pattern[i + 2])
    )
      return true
  }
  return false
}

function codexCleanSchemaPatterns(schema) {
  if (!schema || typeof schema !== 'object' || Array.isArray(schema)) return
  if (codexUnsupportedSchemaPattern(schema.pattern)) delete schema.pattern
  for (const [key, value] of Object.entries(schema)) {
    if (
      [
        'properties',
        'patternProperties',
        '$defs',
        'definitions',
        'dependencies',
        'dependentSchemas',
      ].includes(key)
    ) {
      if (!value || typeof value !== 'object' || Array.isArray(value)) continue
      for (const [name, child] of Object.entries(value)) {
        if (key === 'patternProperties' && codexUnsupportedSchemaPattern(name)) {
          delete value[name]
          if (Object.hasOwn(schema, 'additionalProperties')) schema.additionalProperties = true
        } else codexCleanSchemaPatterns(child)
      }
    } else if (
      ['anyOf', 'allOf', 'oneOf', 'prefixItems', 'items'].includes(key) &&
      Array.isArray(value)
    ) {
      value.forEach(codexCleanSchemaPatterns)
    } else if (
      [
        'items',
        'additionalProperties',
        'additionalItems',
        'contains',
        'not',
        'if',
        'then',
        'else',
        'propertyNames',
        'unevaluatedProperties',
        'unevaluatedItems',
        'contentSchema',
      ].includes(key)
    ) {
      codexCleanSchemaPatterns(value)
    }
  }
}

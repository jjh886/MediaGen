function agPayload(request, auth, selection, requestId, mimeTypes) {
  const instructions = [request.instructions].filter(Boolean)
  const contents = []
  for (const item of request.input) {
    if (item.role === 'tool' || request.tools.length)
      throw new Error('invalid_request: 此接口不支持工具调用')
    if (item.role === 'developer' || item.role === 'system') {
      if (item.attachments?.length) throw new Error('invalid_request: 系统指令不支持附件')
      instructions.push(item.text)
    } else {
      const parts = item.text ? [{ text: item.text }] : []
      for (const file of item.attachments || []) {
        if (!file.base64 || !mimeTypes.includes(file.mime_type))
          throw new Error('invalid_request: 所选模型和推理档位不支持此附件格式')
        parts.push({ inlineData: { mimeType: file.mime_type, data: file.base64 } })
      }
      const role = item.role === 'assistant' ? 'model' : 'user'
      if (contents.at(-1)?.role === role) contents.at(-1).parts.push(...parts)
      else contents.push({ role, parts })
    }
  }
  if (!contents.length) throw new Error('invalid_request: 缺少对话内容')
  const generationConfig = {}
  if (request.max_output_tokens && selection.model.startsWith('claude-'))
    generationConfig.maxOutputTokens = request.max_output_tokens
  if (selection.level && selection.model.startsWith('gemini-4-'))
    generationConfig.thinkingConfig = { thinkingLevel: selection.level, includeThoughts: true }
  const firstUserText = contents.find((item) => item.role === 'user' && item.parts[0]?.text)
    ?.parts[0].text
  const digest = host.call('crypto.sha256_hex', {
    text: request.session_key || firstUserText || requestId,
  }).value
  // 显式会话键沿用已有 ID，以便已建立的会话继续使用同一上游会话。
  const generatedId = request.session_key
    ? '-' + parseInt(digest.slice(0, 13), 16)
    : '-' + (BigInt('0x' + digest.slice(0, 16)) & 0x7fffffffffffffffn).toString()
  const savedId = request.session?.conversation_id
  return {
    model: selection.model,
    project: auth.project,
    requestType: 'agent',
    requestId: 'agent-' + requestId,
    userAgent: 'antigravity',
    request: {
      sessionId: typeof savedId === 'string' && /^-\d{1,19}$/.test(savedId) ? savedId : generatedId,
      contents,
      generationConfig,
      ...(instructions.length
        ? { systemInstruction: { parts: [{ text: instructions.join('\n\n') }] } }
        : {}),
    },
  }
}

function agFinishResponse(writer, reason) {
  const status = reason === 'MAX_TOKENS' ? 'incomplete' : 'completed'
  const item = writer.message
  if (item) {
    const position = { output_index: 0, content_index: 0, item_id: item.id }
    const part = { type: 'output_text', text: writer.text, annotations: [], logprobs: [] }
    apiTextEmitResponseEvent(writer, 'response.output_text.done', {
      ...position,
      text: writer.text,
      logprobs: [],
    })
    apiTextEmitResponseEvent(writer, 'response.content_part.done', { ...position, part })
    item.content = [part]
    item.status = status
    apiTextEmitResponseEvent(writer, 'response.output_item.done', { output_index: 0, item })
  } else if (status === 'completed')
    throw new Error('generation_incomplete: Antigravity 未返回文本')
  const response = apiTextResponseObject(writer, status)
  response.usage = writer.usage || null
  if (status === 'incomplete') response.incomplete_details = { reason: 'max_output_tokens' }
  apiTextEmitResponseEvent(writer, 'response.' + status, { response })
  return response
}

function main(ctx) {
  const saved = host.checkpoint()
  if (saved?.phase === 'completed') return saved.result
  if (saved?.phase === 'submitting')
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 上次请求已发送，无法确认结果')
  const request = apiTextNormalizeTextRequest(ctx.request)
  const slug =
    Object.keys(AG_ORCHESTRATORS).find((key) => AG_ORCHESTRATORS[key] === request.model) ||
    request.model.replace(/^antigravity-/, '')
  if (!AG_MODELS.includes(slug)) throw new Error('invalid_request: 不支持该 Antigravity 模型')
  host.checkpoint({ phase: 'ready' })
  const { auth, models } = agCatalog(ctx, agAuth(ctx))
  const selection = agSelect(slug, models, request.reasoning.effort)
  const id = host.call('crypto.random_uuid', {}).value
  const body = agPayload(request, auth, selection, id, agMimeTypes(models[selection.model]))
  const writer = apiTextCreateResponseWriter(request)
  writer.metadata = { platform_session: { conversation_id: body.request.sessionId } }
  let started = false
  let finish = ''
  function consume(event) {
    const data = event.response || event
    const error = event.error || data.error
    if (error) {
      const code = Number(error.code)
      const status = Number.isInteger(code) && code >= 400 && code <= 599 ? code : 502
      agRequireOk({ status, body: { error } }, 'Antigravity 生成')
    }
    const reason = data.promptFeedback?.blockReason
    if (reason && reason !== 'BLOCK_REASON_UNSPECIFIED')
      throw new Error('content_filter: Antigravity 拒绝了此输入')
    const candidate = data.candidates?.[0]
    if (candidate) {
      if (!started) {
        apiTextStartResponse(writer)
        started = true
      }
      for (const part of candidate.content?.parts || []) {
        if (!part.thought && typeof part.text === 'string') apiTextEmitTextDelta(writer, part.text)
      }
      if (candidate.finishReason) finish = candidate.finishReason
    }
    const usage = data.usageMetadata
    if (usage && Number.isSafeInteger(usage.totalTokenCount)) {
      const input = usage.promptTokenCount || 0
      writer.usage = {
        input_tokens: input,
        output_tokens: Math.max(0, usage.totalTokenCount - input),
        total_tokens: usage.totalTokenCount,
        input_tokens_details: { cached_tokens: usage.cachedContentTokenCount || 0 },
        output_tokens_details: { reasoning_tokens: usage.thoughtsTokenCount || 0 },
      }
    }
  }
  host.checkpoint({ phase: 'submitting' })
  try {
    // AIClient2API 仅对这里的 Claude 型号合并 SSE 作为普通响应。
    const useStream = request.stream || slug.startsWith('claude-')
    const action = useStream ? 'streamGenerateContent?alt=sse' : 'generateContent'
    const params = agParams(auth, agModelEndpoint(auth) + action, body)
    params.headers.Accept = useStream ? 'text/event-stream' : 'application/json'
    params.timeout_ms = 600000
    const result = useStream
      ? host.stream('http.sse_request', params, consume)
      : host.call('http.request', params)
    if (!result.ok && !started && [400, 401, 403, 404, 429].includes(result.status))
      host.checkpoint({ phase: 'ready' })
    const responseBody = agRequireOk(result, 'Antigravity 生成')
    if (!useStream && responseBody) consume(responseBody)
    if (!finish) throw new Error('generation_incomplete: Antigravity 连接结束前未返回完成标记')
    if (!['STOP', 'MAX_TOKENS'].includes(finish))
      throw new Error('generation_incomplete: Antigravity 未正常完成生成')
    const response = agFinishResponse(writer, finish)
    const output = { success: true, status: 'done', source: 'antigravity', response }
    host.checkpoint({ phase: 'completed', result: output })
    return output
  } catch (error) {
    // 上游已明确拒绝该凭据；刷新供后续请求使用，避免在这里重发生成请求。
    if (!started && String(error.message).startsWith('LOGIN_EXPIRED: Antigravity 生成 HTTP 401')) {
      host.checkpoint({ phase: 'ready' })
      agAuth(ctx, auth.access_token)
      throw new Error('upstream_error: Antigravity 本次授权被拒绝，账号凭据已刷新')
    }
    if (started) apiTextFailResponse(writer, error)
    throw error
  }
}

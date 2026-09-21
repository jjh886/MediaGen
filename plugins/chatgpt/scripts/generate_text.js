function main(ctx) {
  const request = chatgptApiNormalizeTextRequest(ctx.request)
  const saved = chatgptApiCheckpoint()
  if (saved.phase === 'completed' && saved.result?.response) return saved.result
  if (!saved.phase || saved.phase === 'ready') chatgptApiCheckpoint({ phase: 'ready' })
  const auth = chatgptApiRefreshSession(chatgptApiReadAuth(ctx))
  if (!saved.conversation_id && saved.phase !== 'submitting')
    chatgptApiTextReasoning(ctx, auth, request)
  const response = chatgptApiGenerateTextConversation(auth, request)
  return {
    success: true,
    status: 'done',
    source: 'chatgpt',
    response,
  }
}

// 提交前失败可换号；已获得会话编号后只能恢复原会话，不能重新发送。
function chatgptApiGenerateTextConversation(auth, request) {
  const saved = chatgptApiCheckpoint()
  if (saved.phase === 'completed' && saved.result?.response) return saved.result.response
  if (saved.phase === 'submitting' && !saved.conversation_id) {
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 原文本会话提交状态待核对')
  }
  const writer = chatgptApiCreateResponseWriter(request)
  try {
    let conversationId = saved.conversation_id || ''
    const parentId = saved.parent_message_id || 'client-created-root'
    const state = chatgptApiCreateTextStreamState()
    if (conversationId) {
      writer.request.model = saved.model || request.model
      chatgptApiStartResponse(writer)
      if (saved.resume_token) {
        chatgptApiRequireOk(
          chatgptApiStreamWithResume(
            auth,
            chatgptApiResumeRequest(auth, conversationId, saved.resume_token, 660000),
            (event) => chatgptApiConsumeTextStreamEvent(state, writer, event)
          ),
          '恢复 ChatGPT 文本结果'
        )
      }
    } else {
      chatgptApiCheckpoint({ phase: 'ready' })
      const references = chatgptApiUploadTextImages(auth, request)
      const prompt = chatgptApiBuildResponsesPrompt(request)
      const thread = { conversation_id: '', parent_message_id: parentId }
      const init = chatgptApiInitializeConversation(auth)
      const options = { ...chatgptApiTextOptions(init, request), references }
      writer.request.model = options.model
      chatgptApiStartResponse(writer)
      const sentinelHeaders = chatgptApiSentinelHeaders(auth)
      const prepared = chatgptApiPrepareTextConversation(
        auth,
        prompt,
        thread,
        options,
        sentinelHeaders
      )
      const checkpoint = {
        phase: 'submitting',
        model: options.model,
        parent_message_id: parentId,
        user_message_id: prepared.user_message_id,
      }
      chatgptApiCheckpoint(checkpoint)
      const result = chatgptApiStreamTextConversation(
        auth,
        prompt,
        prepared,
        sentinelHeaders,
        (event) => {
          chatgptApiConsumeTextStreamEvent(state, writer, event, (id) => {
            if (conversationId) return
            conversationId = id
            chatgptApiCheckpoint({
              ...checkpoint,
              phase: 'accepted',
              conversation_id: conversationId,
            })
          })
        }
      )
      chatgptApiRequireOk(result, '发送 ChatGPT 文本消息')
      conversationId =
        conversationId ||
        chatgptApiFindConversationId(result.events) ||
        chatgptApiFindConversationId(result.body)
      if (!conversationId) {
        throw new Error('PROVIDER_SUBMISSION_UNKNOWN: ChatGPT 文本响应缺少 conversation_id')
      }
      chatgptApiCheckpoint({
        ...chatgptApiCheckpoint(),
        ...checkpoint,
        phase: 'accepted',
        conversation_id: conversationId,
      })
    }
    const finalText =
      state.finished && chatgptApiIsFinalText(state) && state.text.trim()
        ? state.text
        : chatgptApiWaitForText(auth, conversationId, parentId)
    chatgptApiAppendMissingFinalText(state, writer, finalText)
    const response = chatgptApiCompleteResponse(writer)
    // 先保存正文再清理远端会话，清理失败不能丢失已生成结果。
    chatgptApiCheckpoint({
      phase: 'completed',
      conversation_id: conversationId,
      result: { success: true, status: 'done', source: 'chatgpt', response },
    })
    chatgptApiHideConversation(auth, conversationId)
    return response
  } catch (error) {
    chatgptApiFailResponse(writer, error)
    throw error
  }
}

function chatgptApiNormalizeTextRequest(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('invalid_input: 文本模型 request 必须是对象。')
  }
  const options = value.options === undefined ? {} : value.options
  if (!options || typeof options !== 'object' || Array.isArray(options)) {
    throw new Error('invalid_input: 文本模型 request.options 必须是对象。')
  }
  const input = chatgptApiNormalizeResponsesInput(value.input)
  if (!input.length) throw new Error('missing_input: 文本模型 request 缺少 input。')
  const maxOutputTokens = Number(value.max_output_tokens ?? options.max_output_tokens ?? 0)
  if (!Number.isInteger(maxOutputTokens) || maxOutputTokens < 0) {
    throw new Error('invalid_input: max_output_tokens 必须是非负整数。')
  }
  const reasoningEffort = chatgptApiNormalizeReasoningEffort(
    value.reasoning?.effort ?? options.reasoning_effort
  )
  const tools = chatgptApiNormalizeResponsesTools(value.tools)
  const toolChoice = chatgptApiNormalizeToolChoice(value.tool_choice, tools)
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

function chatgptApiNormalizeReasoningEffort(value) {
  return String(value || '')
    .trim()
    .toLowerCase()
}

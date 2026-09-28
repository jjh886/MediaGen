function main(ctx) {
  const request = apiTextNormalizeTextRequest(ctx.request)
  const saved = chatgptApiCheckpoint()
  if (saved.phase === 'completed' && saved.result?.response) return saved.result
  if (!saved.phase) chatgptApiCheckpoint({ phase: 'ready' })
  const auth = chatgptApiRefreshSession(chatgptApiReadAuth(ctx))
  if (
    !saved.conversation_id &&
    saved.phase !== 'submitting' &&
    request.input.some((item) => item.attachments?.length)
  ) {
    request.attachments = Array.isArray(saved.attachments)
      ? saved.attachments
      : chatgptApiUploadTextAttachments(ctx, auth, request.input)
    chatgptApiCheckpoint({
      ...chatgptApiCheckpoint(),
      phase: 'ready',
      attachments: request.attachments,
    })
  }
  if (!saved.conversation_id && saved.phase !== 'submitting')
    chatgptApiTextReasoning(ctx, auth, request)
  const response = chatgptApiGenerateTextConversation(auth, request)
  return {
    success: true,
    status: 'done',
    source: 'chatgpt-api',
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
  const writer = apiTextCreateResponseWriter(request)
  try {
    let conversationId = saved.conversation_id || ''
    const parentId = saved.parent_message_id || 'client-created-root'
    const state = chatgptApiCreateTextStreamState()
    if (conversationId) {
      writer.request.model = saved.model || request.model
      apiTextStartResponse(writer)
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
      chatgptApiCheckpoint({ ...chatgptApiCheckpoint(), phase: 'ready' })
      const prompt = apiTextBuildResponsesPrompt(request)
      const thread = { conversation_id: '', parent_message_id: parentId }
      const init = chatgptApiInitializeConversation(auth)
      const options = chatgptApiTextOptions(init, request)
      writer.request.model = options.model
      apiTextStartResponse(writer)
      const sentinelHeaders = chatgptApiSentinelHeaders(auth)
      const prepared = chatgptApiPrepareTextConversation(
        auth,
        prompt,
        thread,
        options,
        sentinelHeaders,
        request.attachments || saved.attachments || []
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
        request.attachments || saved.attachments || [],
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
    apiTextReadConversationTitle(
      writer,
      conversationId,
      () => chatgptApiFetchTextConversation(auth, conversationId, 3000).title || ''
    )
    const response = apiTextCompleteResponse(writer)
    // 先保存正文再清理远端会话，清理失败不能丢失已生成结果。
    chatgptApiCheckpoint({
      phase: 'completed',
      conversation_id: conversationId,
      result: { success: true, status: 'done', source: 'chatgpt-api', response },
    })
    chatgptApiHideConversation(auth, conversationId)
    return response
  } catch (error) {
    apiTextFailResponse(writer, error)
    throw error
  }
}

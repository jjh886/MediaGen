function main(ctx) {
  const fixedVideoAnalysis = ctx.fixed_video_analysis ?? ctx.request?.options?.fixed_video_analysis
  if (fixedVideoAnalysis !== undefined && typeof fixedVideoAnalysis !== 'boolean')
    throw new Error('invalid_input: fixed_video_analysis 必须是布尔值')
  chatgptApiFixedVideoAnalysis = fixedVideoAnalysis === true
  let request = apiTextNormalizeTextRequest(ctx.request)
  const saved = chatgptApiPrepareConversationAttempt(request.session)
  if (saved.phase === 'completed' && saved.result?.response) return saved.result
  if (request.session?.conversation_id && !saved.reset_session)
    request = apiTextSessionRequest(request)
  if (!saved.phase) chatgptApiCheckpoint({ ...saved, phase: 'ready' })
  if (saved.reset_session && request.session && !saved.session_attachments_rebuilt)
    delete saved.attachments
  const auth = chatgptApiRefreshSession(chatgptApiReadAuth(ctx))
  const bootstrap =
    !saved.conversation_id && saved.phase !== 'submitting' ? chatgptApiBootstrap(auth, false) : null
  if (
    !saved.conversation_id &&
    saved.phase !== 'submitting' &&
    request.input.some((item) => item.attachments?.length)
  ) {
    if (
      Array.isArray(saved.attachments) &&
      request.input.some((item) =>
        item.attachments?.some((attachment) => attachment.mime_type.startsWith('audio/'))
      )
    )
      chatgptApiTranscribeAttachments(auth, request.input, ctx)
    request.attachments = Array.isArray(saved.attachments)
      ? saved.attachments
      : chatgptApiUploadTextAttachments(ctx, auth, request.input)
    chatgptApiCheckpoint({
      ...chatgptApiCheckpoint(),
      phase: 'ready',
      attachments: request.attachments,
      session_attachments_rebuilt: !!(saved.reset_session && request.session),
    })
  }
  if (!saved.conversation_id && saved.phase !== 'submitting')
    chatgptApiTextReasoning(ctx, auth, request, bootstrap['/backend-api/models'])
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
  let started = false
  const start = () => {
    if (!started) {
      started = true
      apiTextStartResponse(writer)
    }
  }
  let connection
  try {
    if (!saved.conversation_id) connection = chatgptApiTryOpenTextUpdates(auth)
    let conversationId = saved.conversation_id || ''
    const continued =
      !saved.reset_session && request.session?.conversation_id ? request.session : null
    let parentId = saved.parent_message_id || continued?.parent_message_id || 'client-created-root'
    if (!saved.conversation_id && continued && !continued.parent_message_id) {
      const snapshot = chatgptApiTextConversationResponse(auth, continued.conversation_id)
      chatgptApiRequireRetryableRead(snapshot, continued.conversation_id)
      const conversation = chatgptApiRequireOk(snapshot, '读取原会话最后回复').body || {}
      parentId =
        conversation.current_node ||
        chatgptApiCurrentTurnMessages(conversation, '', 'client-created-root')
          .filter(
            (message) =>
              message.author?.role === 'assistant' && chatgptApiTextMessageFinished(message)
          )
          .at(-1)?.id
      if (!parentId) throw new Error('temporary_unavailable: 原会话尚未取得最后回复编号')
    }
    const state = chatgptApiCreateTextStreamState()
    if (conversationId) {
      writer.request.model = saved.model || request.model
      start()
      // 恢复先查询原会话快照；旧实时流可能已经关闭，不能先等待过期流。
    } else {
      chatgptApiCheckpoint({ ...chatgptApiCheckpoint(), phase: 'ready' })
      const prompt = apiTextBuildResponsesPrompt(request)
      const thread = {
        conversation_id: continued?.conversation_id || '',
        parent_message_id: parentId,
      }
      const init = chatgptApiInitializeConversation(auth)
      const options = chatgptApiTextOptions(init, request)
      writer.request.model = options.model
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
        ...(chatgptApiFixedVideoAnalysis ? { fixed_video_wait_started_at_ms: Date.now() } : {}),
        ...(continued ? { continuation_id: continued.conversation_id } : {}),
        reset_session: saved.reset_session === true,
        ...(continued ? { conversation_id: continued.conversation_id } : {}),
        model: options.model,
        parent_message_id: parentId,
        user_message_id: prepared.user_message_id,
        attachments: request.attachments || saved.attachments || [],
      }
      chatgptApiCheckpoint(checkpoint)
      const rememberConversation = (id) => {
        if (continued && id !== continued.conversation_id)
          throw new Error('invalid_response: 平台返回的会话与当前窗口绑定不一致')
        if (conversationId) return
        conversationId = id
        chatgptApiCheckpoint({ ...checkpoint, phase: 'accepted', conversation_id: id })
      }
      const result = chatgptApiStreamTextConversation(
        auth,
        prompt,
        prepared,
        sentinelHeaders,
        request.attachments || saved.attachments || [],
        (event) => {
          const receivedId =
            chatgptApiFindConversationId(event) ||
            (event?.p === '/conversation_id' && typeof event.v === 'string' ? event.v : '')
          if (receivedId) rememberConversation(receivedId)
          start()
          chatgptApiConsumeTextStreamEvent(state, writer, event, rememberConversation)
        }
      )
      if (chatgptApiPollRateLimited(result) && chatgptApiCheckpoint().phase === 'submitting') {
        const retry = { ...chatgptApiCheckpoint(), phase: 'ready' }
        delete retry.conversation_id
        chatgptApiCheckpoint(retry)
        throw new Error('rate_limit_exceeded: HTTP 429 当前消息未被受理，请重试')
      }
      if (!(state.finished && chatgptApiIsFinalText(state) && state.text.trim())) {
        chatgptApiRequireRetryableRead(result)
        chatgptApiRequireTextStreamOrPoll(result, '发送 ChatGPT 文本消息', conversationId)
      }
      conversationId =
        conversationId ||
        chatgptApiFindConversationId(result.events) ||
        chatgptApiFindConversationId(result.body) ||
        continued?.conversation_id
      if (!conversationId) {
        throw new Error('PROVIDER_SUBMISSION_UNKNOWN: ChatGPT 文本响应缺少 conversation_id')
      }
      chatgptApiCheckpoint({
        ...chatgptApiCheckpoint(),
        ...checkpoint,
        phase: 'accepted',
        conversation_id: conversationId,
      })
      start()
    }
    const streamedImages = chatgptApiResolveTextImages(auth, conversationId, state, writer)
    const finalText =
      streamedImages && !state.pending_image_files
        ? state.final_text || ''
        : !state.image_pending &&
            state.finished &&
            chatgptApiIsFinalText(state) &&
            state.text.trim()
          ? state.text
          : chatgptApiWaitForRichText(auth, conversationId, parentId, state, writer, connection)
    chatgptApiAppendMissingFinalText(state, writer, finalText)
    apiTextReadConversationTitle(
      writer,
      conversationId,
      () => chatgptApiFetchTextConversation(auth, conversationId, 3000).title || ''
    )
    if (request.session_key)
      writer.metadata = {
        ...writer.metadata,
        platform_session: {
          conversation_id: conversationId,
          parent_message_id: state.message_id || '',
        },
      }
    const response = apiTextCompleteResponse(writer)
    // 自由文本会话保留在平台侧，便于核对结果、标题和后续恢复。
    chatgptApiCheckpoint({
      phase: 'completed',
      conversation_id: conversationId,
      result: { success: true, status: 'done', source: 'chatgpt-api', response },
    })
    return response
  } catch (error) {
    if (started) apiTextFailResponse(writer, error)
    throw error
  } finally {
    chatgptApiCloseTextUpdates(connection)
  }
}

// 已接收会话的流可能在完成后过期；读取原会话正文，不重新提交消息。
function chatgptApiRequireTextStreamOrPoll(result, operation, conversationId) {
  chatgptApiRequireRetryableRead(result, conversationId)
  if (!result?.ok && [404, 429].includes(Number(result?.status)) && conversationId) {
    chatgptApiCheckpoint({ ...chatgptApiCheckpoint(), resume_token: '' })
    return
  }
  chatgptApiRequireOk(result, operation)
}

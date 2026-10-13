function main(ctx) {
  let request = apiTextNormalizeTextRequest(ctx.request)
  const saved = host.checkpoint() || {}
  if (
    saved.phase === 'completed' &&
    !(saved.result?.response?.output || []).some((item) => item.type === 'video_generation_call')
  )
    return saved.result

  const continued =
    !saved.reset_session && request.session?.conversation_id ? request.session : null
  if (continued) request = apiTextSessionRequest(request)
  const session = apiSessionRead(ctx)
  doubaoRequireVerified(session)
  doubaoStartHeartbeats(session)
  if (saved.phase === 'completed') return doubaoRefreshCompletedTextMedia(session, saved.result)
  const writer = apiTextCreateResponseWriter(request)
  const media = doubaoTextMediaState()
  media.pending = saved.media_kind || ''
  apiTextStartResponse(writer)
  try {
    let conversationId = saved.conversation_id
    if (conversationId) {
      if (media.pending) apiTextMediaProgress(writer, 'pending', media.pending, 'in_progress')
      doubaoWaitForTextMedia(session, media, writer, conversationId)
    } else {
      const references = doubaoUploadTextAttachments(ctx, session, request)
      const options = {
        reasoning: request.reasoning.effort,
        references: doubaoReferenceBlocks(references),
        reference_message_id: references[0]?.message_id,
      }
      let payload
      if (continued) {
        const info = doubaoImRequest(session, '/im/conversation/info', 1110, {
          get_conv_info_uplink_body: {
            conversation_id: continued.conversation_id,
            ext: { cold_start: 'true' },
            bot_id: '',
            conversation_type: 3,
            option: { need_bot_info: true },
          },
        })
        const conversation = info.downlink_body?.get_conv_info_downlink_body?.conversation_info
        if (String(conversation?.conversation_id) !== String(continued.conversation_id))
          throw new Error('temporary_unavailable: 原豆包会话无法确认')
        payload = doubaoContinuationPayload(
          conversation,
          apiTextBuildResponsesPrompt(request),
          options
        )
      } else payload = doubaoPayload(apiTextBuildResponsesPrompt(request), 'text', options)
      const state = doubaoSubmit(
        session,
        payload,
        (text) => apiTextFeedResponseText(writer, text),
        (stream, event) => doubaoTextMediaEvent(session, media, writer, stream, event)
      )
      if (continued && String(state.conversationId) !== String(continued.conversation_id))
        throw new Error('invalid_response: 平台返回的会话与当前窗口绑定不一致')
      conversationId = state.conversationId
      const creations = doubaoCreations(state)
      if (media.pending || creations.length) {
        doubaoCompleteTextMedia(session, media, writer, conversationId, creations)
        if (
          !creations.length ||
          creations.some((creation) => !media.completed[doubaoTextCreationKey(creation)])
        )
          doubaoWaitForTextMedia(session, media, writer, conversationId)
      } else if (!state.text.trim()) throw new Error('invalid_response: 豆包未返回正文')
    }
    doubaoMarkConversationRead(session, conversationId)
    apiTextReadConversationTitle(writer, conversationId, () =>
      doubaoReadTextTitle(session, conversationId)
    )
    for (const id of Object.keys(writer.reasoning_items || {}))
      apiTextUpdateReasoning(writer, id, '', true)
    if (request.session_key)
      writer.metadata = {
        ...writer.metadata,
        platform_session: { conversation_id: conversationId },
      }
    const response = apiTextCompleteResponse(writer)
    const result = { success: true, status: 'done', source: 'doubao-api', response }
    host.checkpoint({ phase: 'completed', conversation_id: conversationId, result })
    return result
  } catch (error) {
    apiTextFailResponse(writer, error)
    throw error
  }
}

function doubaoReadTextTitle(session, conversationId) {
  const info = doubaoImRequest(
    session,
    '/im/conversation/info',
    1110,
    {
      get_conv_info_uplink_body: {
        conversation_id: conversationId,
        ext: { cold_start: 'true' },
        bot_id: '',
        conversation_type: 3,
        option: { need_bot_info: true },
      },
    },
    3000
  )
  const conversation = info.downlink_body?.get_conv_info_downlink_body?.conversation_info
  return String(conversation?.conversation_id) === String(conversationId)
    ? conversation.name || ''
    : ''
}

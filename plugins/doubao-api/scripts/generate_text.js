function main(ctx) {
  const request = apiTextNormalizeTextRequest(ctx.request)
  const saved = host.checkpoint() || {}
  if (
    saved.phase === 'completed' &&
    !(saved.result?.response?.output || []).some((item) => item.type === 'video_generation_call')
  )
    return saved.result
  if (saved.phase === 'submitting' && !saved.conversation_id) {
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 请恢复原豆包会话，不能重复提交')
  }
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
      const state = doubaoSubmit(
        session,
        doubaoPayload(apiTextBuildResponsesPrompt(request), 'text', {
          reasoning: request.reasoning.effort,
          references: doubaoReferenceBlocks(references),
          reference_message_id: references[0]?.message_id,
        }),
        (text) => apiTextFeedResponseText(writer, text),
        (stream, event) => doubaoTextMediaEvent(session, media, writer, stream, event)
      )
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

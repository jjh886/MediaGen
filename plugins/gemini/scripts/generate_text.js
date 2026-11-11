function main(ctx) {
  let request = apiTextNormalizeTextRequest(ctx.request)
  const saved = host.checkpoint() || {}
  if (saved.phase === 'completed') return saved.result
  if (saved.phase === 'failed') throw new Error(saved.error)
  const session = geminiInitialize(apiSessionRead(ctx))
  const model = geminiModel(session, request.model, request.reasoning.effort)
  const writer = apiTextCreateResponseWriter(request)
  let started = false
  const emit = (result) => {
    if (!started && (result.text || result.reasoning)) {
      apiTextStartResponse(writer)
      started = true
    }
    if (result.reasoning) apiTextUpdateReasoning(writer, 'thinking', result.reasoning, false)
    if (result.text.startsWith(writer.text))
      apiTextFeedResponseText(writer, result.text.slice(writer.text.length))
  }
  try {
    let result
    if (saved.conversation_id && saved.response_id && saved.phase === 'accepted') {
      const history = geminiHistory(session, saved)
      if (!history?.text) throw new Error('temporary_unavailable: 原 Gemini 会话结果尚未就绪')
      result = { ...saved, ...history }
      emit(result)
    } else {
      if (saved.phase === 'submitting')
        throw new Error('PROVIDER_SUBMISSION_UNKNOWN: Gemini 提交状态未知，禁止重复发送')
      const previous = request.session
      if (previous) request = apiTextSessionRequest(request)
      host.checkpoint({ ...saved, phase: 'ready' })
      const files = geminiReferences(ctx, session, request)
      result = geminiGenerate(
        session,
        model,
        apiTextBuildResponsesPrompt(request),
        files,
        previous,
        null,
        emit
      )
    }
    if (!result.text) throw new Error('invalid_response: Gemini 完成但未返回正文')
    if (!started) apiTextStartResponse(writer)
    if (writer.reasoning_items?.thinking)
      apiTextUpdateReasoning(writer, 'thinking', result.reasoning || '', true)
    if (request.session_key)
      writer.metadata = {
        platform_session: {
          conversation_id: result.conversation_id,
          response_id: result.response_id,
          candidate_id: result.candidate_id,
          token: result.token,
          turn: host.checkpoint()?.turn || 1,
        },
      }
    const output = { success: true, status: 'done', response: apiTextCompleteResponse(writer) }
    host.checkpoint({ ...host.checkpoint(), phase: 'completed', result: output })
    return output
  } catch (error) {
    if (started) apiTextFailResponse(writer, error)
    throw error
  }
}

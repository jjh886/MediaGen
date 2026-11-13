function main(ctx) {
  let request = apiTextNormalizeTextRequest(ctx.request)
  const saved = host.checkpoint() || {}
  if (saved.phase === 'completed') return saved.result
  if (saved.phase === 'failed') throw new Error(saved.error)
  const session = geminiInitialize(apiSessionRead(ctx))
  const model =
    saved.phase === 'accepted'
      ? null
      : geminiModel(
          session,
          saved.fallback_model || request.model,
          saved.fallback_model ? undefined : request.reasoning.effort
        )
  const requestedModel = request.model
  const writer = apiTextCreateResponseWriter(request)
  let started = false
  const emit = (result) => {
    const reported = result.actual_model_name || ''
    const actualId =
      reported &&
      'gemini-' +
        reported
          .replace(/\s+(?:low|medium|high)$/i, '')
          .toLowerCase()
          .replace(/[^a-z0-9.]+/g, '-')
          .replace(/^-+|-+$/g, '')
    if (actualId) writer.request.model = actualId
    if (!started && (result.text || result.reasoning)) {
      apiTextStartResponse(writer)
      started = true
    }
    if (result.reasoning) apiTextUpdateReasoning(writer, 'thinking', result.reasoning, false)
    if (result.text.startsWith(writer.text))
      apiTextFeedResponseText(writer, result.text.slice(writer.text.length))
  }
  const recover = (current) => {
    let history
    try {
      history = geminiHistory(session, current)
    } catch (error) {
      if (!current.channel_ended || !String(error.message).startsWith('conversation_unavailable:'))
        throw error
    }
    if (history?.text) return { ...current, ...history }
    if (!current.channel_ended) throw new Error('temporary_unavailable: 原 Gemini 会话结果尚未就绪')
    if (current.fallback_model || current.received_text_length)
      throw new Error('generation_failed: Gemini 上游已结束生成，但原轮次没有完整结果')
    const base = geminiCatalog(session).find(
      (candidate) => candidate.mode === 1 && candidate.id !== requestedModel
    )
    if (!base) throw new Error('generation_failed: Gemini 上游未返回结果，账号也没有可用的基础模型')
    // 仅原通道明确结束且没有正文时降级一次；保存原回执，断线后不重复提交。
    host.checkpoint({
      ...current,
      phase: 'ready',
      fallback_model: base.id,
      original_receipt: {
        conversation_id: current.conversation_id,
        response_id: current.response_id,
      },
      conversation_id: '',
      response_id: '',
      candidate_id: '',
      token: '',
      actual_model_name: '',
      channel_ended: false,
      channel_completed: false,
      accepted_at: null,
      recovery_started_at: null,
    })
    const nextRequest = request.session ? apiTextSessionRequest(request) : request
    const files = geminiReferences(ctx, session, nextRequest)
    try {
      const result = geminiGenerate(
        session,
        { ...base, think: 1, tool: 0 },
        apiTextBuildResponsesPrompt(nextRequest),
        files,
        request.session,
        null,
        (value) => emit({ ...value, actual_model_name: value.actual_model_name || base.name })
      )
      return { ...result, actual_model_name: result.actual_model_name || base.name }
    } catch (error) {
      const next = host.checkpoint() || {}
      if (next.channel_ended && String(error.message).startsWith('temporary_unavailable:'))
        return recover(next)
      throw error
    }
  }

  try {
    let result
    if (saved.conversation_id && saved.response_id && saved.phase === 'accepted') {
      const recoveryStarted = saved.accepted_at || saved.recovery_started_at || Date.now()
      host.checkpoint({ ...saved, recovery_started_at: recoveryStarted })
      if (Date.now() - recoveryStarted >= 20 * 60 * 1000)
        throw new Error(
          'recovery_exhausted: Gemini 原任务超过恢复时限，已停止自动恢复；请先核对上游原对话'
        )
      result = recover(saved)
      emit(result)
    } else {
      if (saved.phase === 'submitting')
        throw new Error('PROVIDER_SUBMISSION_UNKNOWN: Gemini 提交状态未知，禁止重复发送')
      const previous = request.session
      if (previous) request = apiTextSessionRequest(request)
      host.checkpoint({ ...saved, phase: 'ready' })
      const files = geminiReferences(ctx, session, request)
      try {
        result = geminiGenerate(
          session,
          model,
          apiTextBuildResponsesPrompt(request),
          files,
          previous,
          null,
          emit
        )
      } catch (error) {
        const current = host.checkpoint() || {}
        if (!current.channel_ended || !String(error.message).startsWith('temporary_unavailable:'))
          throw error
        result = recover(current)
        emit(result)
      }
    }
    if (!result.text) throw new Error('invalid_response: Gemini 完成但未返回正文')
    if (!started) apiTextStartResponse(writer)
    if (writer.reasoning_items?.thinking)
      apiTextUpdateReasoning(writer, 'thinking', result.reasoning || '', true)
    if (request.session_key)
      writer.metadata = {
        ...writer.metadata,
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
    if (
      /^(conversation_unavailable|recovery_exhausted|generation_failed):/.test(
        String(error.message)
      )
    )
      host.checkpoint({ ...host.checkpoint(), phase: 'failed', error: String(error.message) })
    if (started) apiTextFailResponse(writer, error)
    throw error
  }
}

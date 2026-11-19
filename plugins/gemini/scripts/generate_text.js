function geminiFailureReply(error) {
  const prefix = 'upstream_generation_failed: '
  const message = String(error.message)
  if (!message.startsWith(prefix)) return ''
  try {
    const reply = JSON.parse(message.slice(prefix.length)).message
    return typeof reply === 'string' ? reply : ''
  } catch (_) {
    return ''
  }
}

function main(ctx) {
  const request = apiTextNormalizeTextRequest(ctx.request)
  const saved = host.checkpoint() || {}
  if (saved.phase === 'completed') return saved.result
  if (saved.phase === 'failed') throw new Error(saved.error)
  if (saved.retry_reply !== undefined && saved.phase !== 'accepted')
    throw new Error(
      saved.original_receipt?.error ||
        'upstream_generation_failed: ' + JSON.stringify({ message: saved.retry_reply })
    )
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
  const progress = []
  let started = false
  let emittedText = null
  let hasOutput = Boolean(saved.has_output || saved.received_text_length)
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
    if (
      !started &&
      (result.text ||
        result.reasoning ||
        result.reasoning_status ||
        (result.conversation_id && result.response_id))
    ) {
      apiTextStartResponse(writer)
      started = true
    }
    // 仅转发上游阶段提示；摘要尚未生成时也能展示真实进度。
    if (result.reasoning_status && progress.at(-1) !== result.reasoning_status) {
      progress.push(result.reasoning_status)
      apiTextUpdateReasoning(writer, 'thinking-progress', progress.join('\n\n'), false)
    }
    if (result.reasoning) apiTextUpdateReasoning(writer, 'thinking', result.reasoning, false)
    if (result.text !== emittedText && result.text.startsWith(writer.text)) {
      apiTextFeedResponseText(writer, result.text.slice(writer.text.length))
      emittedText = result.text
    }
    if (!hasOutput && writer.output.length) {
      hasOutput = true
      host.checkpoint({ ...host.checkpoint(), has_output: true })
    }
  }
  const turnRequest = request.session ? apiTextSessionRequest(request) : request
  let files
  const references = () => (files ||= geminiReferences(ctx, session, turnRequest))
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
    if (current.retry_reply !== undefined || current.fallback_model || current.received_text_length)
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
    try {
      const result = geminiGenerate(
        session,
        { ...base, think: 1, tool: 0 },
        apiTextBuildResponsesPrompt(turnRequest),
        references(),
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

  const firstResult = () => {
    if (saved.conversation_id && saved.response_id && saved.phase === 'accepted') {
      const recoveryStarted = saved.accepted_at || saved.recovery_started_at || Date.now()
      host.checkpoint({ ...saved, recovery_started_at: recoveryStarted })
      if (Date.now() - recoveryStarted >= 20 * 60 * 1000)
        throw new Error(
          'recovery_exhausted: Gemini 原任务超过恢复时限，已停止自动恢复；请先核对上游原对话'
        )
      return recover(saved)
    } else {
      if (saved.phase === 'submitting')
        throw new Error('PROVIDER_SUBMISSION_UNKNOWN: Gemini 提交状态未知，禁止重复发送')
      const previous = request.session
      host.checkpoint({ ...saved, phase: 'ready' })
      try {
        return geminiGenerate(
          session,
          model,
          apiTextBuildResponsesPrompt(turnRequest),
          references(),
          previous,
          null,
          emit
        )
      } catch (error) {
        const current = host.checkpoint() || {}
        if (!current.channel_ended || !String(error.message).startsWith('temporary_unavailable:'))
          throw error
        return recover(current)
      }
    }
  }
  try {
    let result
    try {
      result = firstResult()
    } catch (error) {
      const current = host.checkpoint()
      const marker = current?.failure_frame?.[31]
      if (
        (marker !== true && marker !== 1) ||
        current.phase !== 'failed' ||
        hasOutput ||
        writer.output.length ||
        current.retry_reply !== undefined
      )
        throw error
      // 先持久化一次性重试，再从本轮原父节点提交，避免把失败回答拼进下一轮。
      host.checkpoint({
        phase: 'retry_reserved',
        retry_reply: geminiFailureReply(new Error(current.error)),
        original_receipt: current,
        fallback_model: current.fallback_model,
      })
      const retryModel =
        model && model.id === (current.fallback_model || requestedModel)
          ? model
          : geminiModel(
              session,
              current.fallback_model || requestedModel,
              current.fallback_model ? undefined : request.reasoning.effort
            )
      result = geminiGenerate(
        session,
        retryModel,
        apiTextBuildResponsesPrompt(turnRequest),
        references(),
        request.session,
        null,
        emit
      )
    }
    if (!result.text) throw new Error('invalid_response: Gemini 完成但未返回正文')
    emit(result)
    if (!started) apiTextStartResponse(writer)
    if (writer.reasoning_items?.thinking)
      apiTextUpdateReasoning(writer, 'thinking', result.reasoning || '', true)
    if (writer.reasoning_items?.['thinking-progress'])
      apiTextUpdateReasoning(writer, 'thinking-progress', '', true)
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
    const current = host.checkpoint()
    // 流回调异常可能经宿主包装。失败帧中的错误保留本次尝试的完整原因。
    if (current.phase === 'failed' && current.error) error = new Error(current.error)
    if (
      current.retry_reply !== undefined ||
      /^(conversation_unavailable|recovery_exhausted|generation_failed|upstream_generation_failed):/.test(
        String(error.message)
      )
    )
      host.checkpoint({ ...current, phase: 'failed', error: String(error.message) })
    if (started) apiTextFailResponse(writer, new Error(geminiFailureReply(error) || error.message))
    throw error
  }
}

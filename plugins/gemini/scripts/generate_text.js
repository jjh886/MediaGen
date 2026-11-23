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
  if (saved.reasoning_retry && saved.phase !== 'accepted')
    throw new Error('recovery_exhausted: 仅有思考摘要的原轮次已经补发过，无法确认补发结果')
  const session = geminiInitialize(apiSessionRead(ctx))
  const model =
    saved.phase === 'accepted'
      ? null
      : geminiModel(
          session,
          saved.fallback_model || request.model,
          saved.fallback_model && saved.fallback_model !== request.model
            ? undefined
            : request.reasoning.effort
        )
  const requestedModel = request.model
  const writer = apiTextCreateResponseWriter(request)
  if (ctx.task_id?.startsWith('receipt_')) {
    // 回读历史时复用原响应身份和工具定义；附件、提示词和会话令牌不需要重复存储。
    const { input, session: cursor, ...format } = request
    saved.receipt_writer ||= { id: writer.response_id, created_at: writer.created_at, format }
    writer.response_id = saved.receipt_writer.id
    writer.created_at = saved.receipt_writer.created_at
  }
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
  let historyUpload
  const historyEnd = turnRequest.input.map((item) => item.role).lastIndexOf('assistant') + 1
  const withHistoryFile = () => {
    if (historyUpload) return historyUpload
    const history = turnRequest.input
      .slice(0, historyEnd)
      .map((item) => '[' + item.role.toUpperCase() + ']\n' + item.text)
      .join('\n\n')
    const name = 'conversation-history-' + apiTextUuid() + '.txt'
    const [attachment] = apiTextResponsesMedia([
      {
        type: 'input_file',
        filename: name,
        file_data: host.call('crypto.base64_encode_utf8', { text: history }).value,
      },
    ])
    const reference = geminiUpload(session, attachment)
    historyUpload = {
      prompt: apiTextBuildResponsesPrompt({
        ...turnRequest,
        input: [
          {
            role: 'user',
            text: '附件 ' + name + ' 是本轮之前的完整对话记录，按原顺序作为上下文继续最后的请求。',
          },
          ...turnRequest.input.slice(historyEnd),
        ],
      }),
      files: [reference, ...references()],
    }
    return historyUpload
  }
  const interrupted = (error) =>
    /temporary_unavailable:|unexpected eof|connection reset|broken pipe|timed? ?out|error sending request|error reading a body from connection|incomplete message/i.test(
      String(error.message)
    )
  const hasAnswer = (state) =>
    Boolean(
      emittedText ||
      state.received_text_length ||
      writer.output.some((item) => item.type !== 'reasoning') ||
      (state.has_output && state.received_text_length === undefined && !started)
    )
  const finishReasoning = () => {
    for (const id of Object.keys(writer.reasoning_items || {}))
      apiTextUpdateReasoning(writer, id, '', true)
    writer.reasoning_items = {}
    progress.length = 0
  }
  const recover = (current) => {
    // 已有回执时刷新原轮次；历史可能比生成通道晚写入。
    const delays = [0, 2000, 5000, 10000, 20000, 30000, 45000, 60000]
    for (const [index, delay] of delays.entries()) {
      if (delay) host.call('time.sleep', { milliseconds: delay })
      try {
        const history = geminiHistory(session, current)
        const text = history?.text || ''
        const prefixMatches = !emittedText || text.startsWith(emittedText)
        if (text && text.length >= (current.received_text_length || 0) && prefixMatches)
          return { ...current, ...history }
        current.history_lookup = {
          attempt: index + 1,
          found_turn: Boolean(history),
          history_chars: text.length,
          received_chars: current.received_text_length || 0,
          prefix_matches: prefixMatches,
        }
        host.checkpoint(current)
      } catch (error) {
        const missing = String(error.message).startsWith('conversation_unavailable:')
        current.history_lookup = {
          attempt: index + 1,
          lookup_error: missing
            ? 'conversation_unavailable'
            : interrupted(error)
              ? 'transport'
              : 'provider',
        }
        host.checkpoint(current)
        if (!(current.channel_ended && missing) && !interrupted(error)) throw error
        if (!missing && index === delays.length - 1) throw error
      }
    }
    if (!current.channel_ended) throw new Error('temporary_unavailable: 原 Gemini 会话结果尚未就绪')
    const incomplete = 'generation_failed: incomplete_upstream_turn'
    if (current.retry_reply !== undefined || current.fallback_model || hasAnswer(current))
      throw new Error(incomplete)
    // 只收到思考时保留原模型和思考等级；关闭旧摘要，下一次生成使用独立的摘要事件。
    finishReasoning()
    const base =
      !hasOutput &&
      geminiCatalog(session).find(
        (candidate) => candidate.mode === 1 && candidate.id !== requestedModel
      )
    const nextModel = base
      ? { ...base, think: 1, tool: 0 }
      : model || geminiModel(session, requestedModel, request.reasoning.effort)
    // 通道已关闭且原轮次持续为空，只补发一次；已是基础模型时保留原思考等级。

    host.checkpoint({
      ...current,
      phase: 'ready',
      fallback_model: nextModel.id,
      reasoning_retry: hasOutput,
      original_receipt: {
        conversation_id: current.conversation_id,
        response_id: current.response_id,
        history_lookup: current.history_lookup,
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
    const redo =
      hasOutput && current.history_lookup?.lookup_error !== 'conversation_unavailable'
        ? current
        : undefined
    const result = generate(
      nextModel,
      (value) => emit({ ...value, actual_model_name: value.actual_model_name || nextModel.name }),
      redo
    )

    return { ...result, actual_model_name: result.actual_model_name || nextModel.name }
  }

  const generate = (selected, onResult = emit, redo) => {
    let result
    try {
      const packed = host.checkpoint()?.use_history_file ? withHistoryFile() : null
      result = geminiGenerate(
        session,
        selected,
        packed?.prompt || apiTextBuildResponsesPrompt(turnRequest),
        packed?.files || references(),
        request.session,
        null,
        onResult,
        redo
      )
    } catch (error) {
      const current = host.checkpoint()
      // 仅在长正文已被明确拒绝且尚无回执时，换用 TXT 承载前文。保留原附件及本轮完整输入。
      if (
        current?.phase === 'ready' &&
        current.rejected_message_bytes >= 1024 * 1024 &&
        String(error.message).includes('invalid_input:') &&
        !current.use_history_file &&
        !current.conversation_id &&
        !request.session &&
        !hasOutput &&
        current.retry_reply === undefined &&
        !current.fallback_model &&
        historyEnd > 0 &&
        historyEnd < turnRequest.input.length &&
        references().length < 10
      ) {
        host.checkpoint({ ...current, use_history_file: true, rejected_message_bytes: null })
        return generate(selected, onResult)
      }
      if (
        current?.phase !== 'accepted' ||
        !current.conversation_id ||
        !current.response_id ||
        !interrupted(error)
      )
        throw error
      return recover(current)
    }
    const current = host.checkpoint()
    if (
      !result.text &&
      current?.phase === 'accepted' &&
      current.conversation_id &&
      current.response_id
    )
      return recover(current)
    return result
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
      host.checkpoint({ ...saved, phase: 'ready' })
      return generate(model)
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
        hasAnswer(current) ||
        current.fallback_model ||
        current.retry_reply !== undefined
      )
        throw error
      finishReasoning()
      // 先占用唯一一次 Redo，再短暂等待；断线重连不会再次创建回答版本。
      host.checkpoint({
        phase: 'retry_reserved',
        receipt_writer: current.receipt_writer,
        retry_reply: geminiFailureReply(new Error(current.error)),
        original_receipt: current,
        fallback_model: current.fallback_model,
        use_history_file: current.use_history_file,
      })
      host.call('time.sleep', { milliseconds: 1500 })
      const retryModel =
        model && model.id === (current.fallback_model || requestedModel)
          ? model
          : geminiModel(
              session,
              current.fallback_model || requestedModel,
              current.fallback_model ? undefined : request.reasoning.effort
            )
      result = generate(
        retryModel,
        emit,
        current.conversation_id && current.response_id ? current : undefined
      )
    }
    if (!result.text) throw new Error('invalid_response: Gemini 完成但未返回正文')
    emit(result)
    if (!started) apiTextStartResponse(writer)
    finishReasoning()
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
      (current.reasoning_retry && current.phase === 'ready') ||
      /^(conversation_unavailable|recovery_exhausted|generation_failed|upstream_generation_failed):/.test(
        String(error.message)
      )
    )
      host.checkpoint({ ...current, phase: 'failed', error: String(error.message) })
    if (started) apiTextFailResponse(writer, new Error(geminiFailureReply(error) || error.message))
    throw error
  }
}

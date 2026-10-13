function main(ctx) {
  let request = apiTextNormalizeTextRequest(ctx.request)
  let saved = host.checkpoint() || {}
  if (saved.phase === 'completed') return saved.result

  deepseekValidateAttachments(request)
  const session = apiSessionRead(ctx)
  deepseekInitializeSession(session)
  if (['submitting', 'accepted'].includes(saved.phase) && !saved.response_message_id) {
    // 每次生成创建独立会话。确认只含原始一轮时才认领消息，不能取其他轮次的结果。
    const history = saved.conversation_id
      ? deepseekRequest(
          session,
          '/chat/history_messages?chat_session_id=' + encodeURIComponent(saved.conversation_id),
          undefined,
          'GET'
        )
      : { chat_messages: [] }
    const messages = history.chat_messages
    if (!Array.isArray(messages)) throw new Error('temporary_unavailable: DeepSeek 会话历史未就绪')
    const answers = messages.filter((item) => item.role === 'ASSISTANT')
    const users = messages.filter((item) => item.role === 'USER')
    const message =
      !saved.parent_message_id && answers.length === 1 && users.length <= 1 ? answers[0] : null
    if (Number.isSafeInteger(message?.message_id) && message.message_id > 0) {
      saved = { ...saved, phase: 'accepted', response_message_id: message.message_id }
    } else {
      saved = {
        phase: 'ready',
        reset_session: true,
        attachment_file_ids: saved.attachment_file_ids,
      }
    }
    host.checkpoint(saved)
  }
  const continued =
    !saved.reset_session && request.session?.conversation_id ? request.session : null
  if (continued && !Number.isSafeInteger(continued.parent_message_id)) {
    const history = deepseekRequest(
      session,
      '/chat/history_messages?chat_session_id=' + encodeURIComponent(continued.conversation_id),
      undefined,
      'GET'
    )
    const answers = (history.chat_messages || []).filter(
      (message) => message.role === 'ASSISTANT' && message.status === 'FINISHED'
    )
    if (answers.length !== 1 || !Number.isSafeInteger(answers[0].message_id))
      throw new Error('temporary_unavailable: 旧会话最后回复编号无法确认')
    continued.parent_message_id = answers[0].message_id
  }
  if (continued) request = apiTextSessionRequest(request)
  const resume = Boolean(saved.response_message_id)
  if (saved.reset_session && request.session) delete saved.attachment_file_ids
  const attachments = resume
    ? []
    : saved.attachment_file_ids || deepseekUploadAttachments(ctx, session, request)
  if (!resume)
    host.checkpoint({
      phase: 'ready',
      conversation_id: saved.conversation_id,
      attachment_file_ids: attachments,
    })
  let targetPath = '/api/v0/chat/' + (resume ? 'resume_stream' : 'completion')
  const pow = resume ? null : deepseekPow(session, targetPath)
  let conversationId = saved.conversation_id || continued?.conversation_id
  if (!conversationId) {
    const created = deepseekRequest(session, '/chat_session/create', {})
    conversationId = created.chat_session?.id
    if (!conversationId) throw new Error('invalid_response: DeepSeek 未返回会话编号')
    host.checkpoint({
      phase: 'ready',
      conversation_id: conversationId,
      attachment_file_ids: attachments,
    })
  }
  Object.defineProperty(session, 'conversationId', { value: conversationId })
  const writer = apiTextCreateResponseWriter(request)
  apiTextStartResponse(writer)
  const state = deepseekStreamState()
  const checkpoint = (id) =>
    host.checkpoint({
      phase: 'accepted',
      conversation_id: conversationId,
      response_message_id: id,
    })
  let body = resume
    ? {
        chat_session_id: conversationId,
        message_id: saved.response_message_id,
      }
    : {
        chat_session_id: conversationId,
        parent_message_id: continued?.parent_message_id ?? null,
        model_type: 'default',
        prompt: apiTextBuildResponsesPrompt(request),
        ref_file_ids: attachments,
        thinking_enabled: request.reasoning.effort !== 'none',
        search_enabled: false,
        action: null,
        preempt: false,
      }
  try {
    if (resume) {
      const history = deepseekRequest(
        session,
        '/chat/history_messages?chat_session_id=' + encodeURIComponent(conversationId),
        undefined,
        'GET'
      )
      const message = history.chat_messages?.find(
        (item) => item.message_id === saved.response_message_id && item.role === 'ASSISTANT'
      )
      if (['ERROR', 'FAILED', 'STOPPED'].includes(message?.status)) {
        host.checkpoint({ phase: 'ready', reset_session: true })
        return main(ctx)
      }
      if (message) deepseekSnapshot(state, writer, message)
      state.responseId = saved.response_message_id
    }
    if (!resume)
      host.checkpoint({
        phase: 'submitting',
        conversation_id: conversationId,
        parent_message_id: continued?.parent_message_id,
        reset_session: saved.reset_session === true,
      })
    for (let attempt = 0; !state.finished && attempt < 8; attempt++) {
      const continuing = state.status === 'INCOMPLETE'
      if (attempt > 0 || continuing) {
        if (!state.responseId)
          throw new Error('PROVIDER_SUBMISSION_UNKNOWN: DeepSeek 未返回原消息编号，不能重复提交')
        targetPath = '/api/v0/chat/' + (continuing ? 'continue' : 'resume_stream')
        body = {
          chat_session_id: conversationId,
          message_id: state.responseId,
          ...(continuing ? { fallback_to_resume: true } : {}),
        }
        host.sleep(deepseekSetting(session, 'sse_auto_resume_timeout', 3000))
      }
      const timeoutKey = targetPath.endsWith('/continue')
        ? 'continue_request_timeout_ms'
        : targetPath.endsWith('/resume_stream')
          ? 'resume_request_timeout_ms'
          : 'completion_request_timeout_ms'
      const timeout = deepseekSetting(session, timeoutKey, 60000)
      deepseekReportSettings(session)
      const response = apiSessionRequireOk(
        host.stream(
          'http.sse_request',
          {
            method: 'POST',
            url: DEEPSEEK_ORIGIN + targetPath,
            headers: {
              ...deepseekHeaders(session, targetPath),
              ...(targetPath.endsWith('/completion') ? { 'x-ds-pow-response': pow } : {}),
            },
            body,
            proxy: 'system',
            timeout_ms: timeout,
            idle_interval_ms: 1000,
            retry: { max_attempts: 1 },
            include_sse_metadata: true,
          },
          (event) => {
            deepseekSessionTick(session)
            if (event.type === 'http_idle') {
              return null
            }
            return deepseekConsume(state, writer, event, checkpoint)
          }
        ),
        'DeepSeek 文本生成失败'
      )
      deepseekRefreshSession(session, response, targetPath)
    }
    if (!state.finished) throw new Error('generation_incomplete: DeepSeek 未返回完整正文')
    apiTextReadConversationTitle(writer, conversationId, () =>
      deepseekReadTextTitle(session, conversationId)
    )
    if (request.session_key)
      writer.metadata = {
        ...writer.metadata,
        platform_session: { conversation_id: conversationId, parent_message_id: state.responseId },
      }
    const response = apiTextCompleteResponse(writer)
    const result = { success: true, status: 'done', source: 'deepseek-api', response }
    host.checkpoint({ phase: 'completed', conversation_id: conversationId, result })
    return result
  } catch (error) {
    apiTextFailResponse(writer, error)
    throw error
  }
}

function deepseekReadTextTitle(session, conversationId) {
  const history = deepseekRequest(
    session,
    '/chat/history_messages?chat_session_id=' + encodeURIComponent(conversationId),
    undefined,
    'GET',
    3000
  )
  return history.chat_session?.id === conversationId ? history.chat_session.title || '' : ''
}

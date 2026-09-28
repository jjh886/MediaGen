function main(ctx) {
  const request = apiTextNormalizeTextRequest(ctx.request)
  const saved = host.checkpoint() || {}
  if (saved.phase === 'completed') return saved.result
  if (saved.phase === 'submitting' && !saved.response_message_id) {
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 原 DeepSeek 请求提交状态待核对')
  }
  deepseekValidateAttachments(request)
  const session = apiSessionRead(ctx)
  const resume = Boolean(saved.response_message_id)
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
  let conversationId = saved.conversation_id
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
        parent_message_id: null,
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
      if (message) deepseekSnapshot(state, writer, message)
      state.responseId = saved.response_message_id
    }
    if (!resume) host.checkpoint({ phase: 'submitting', conversation_id: conversationId })
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
        host.sleep(1000)
      }
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
            timeout_ms: 10800000,
            retry: { max_attempts: 1 },
            include_sse_metadata: true,
          },
          (event) => deepseekConsume(state, writer, event, checkpoint)
        ),
        'DeepSeek 文本生成失败'
      )
      deepseekRefreshSession(session, response, targetPath)
    }
    if (!state.finished) throw new Error('generation_incomplete: DeepSeek 未返回完整正文')
    const response = apiTextCompleteResponse(writer)
    const result = { success: true, status: 'done', source: 'deepseek-api', response }
    host.checkpoint({ phase: 'completed', conversation_id: conversationId, result })
    return result
  } catch (error) {
    apiTextFailResponse(writer, error)
    throw error
  }
}

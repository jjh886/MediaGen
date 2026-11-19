function geminiGenerate(session, model, prompt, files, previous, ratio, onResult) {
  const saved = host.checkpoint() || {}
  if (['submitting', 'accepted'].includes(saved.phase))
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 原 Gemini 请求已提交，请恢复原会话')
  let result
  const payload = geminiPayload(prompt, model, files, previous, ratio)
  geminiChannelGenerate(session, model, payload, (frame) => {
    const current = host.checkpoint() || {}
    if (frame[31] === true || frame[31] === 1) {
      const failureContext = {
        requested_model: model.id,
        prompt_chars: prompt.length,
        attachment_count: files.length,
        continued: Boolean(previous),
      }
      let error = geminiGenerationFailure(frame, failureContext)
      host.checkpoint({
        ...current,
        conversation_id: frame[1]?.[0] || current.conversation_id,
        response_id: frame[1]?.[1] || current.response_id,
        phase: 'failed',
        error,
        failure_frame: frame,
        failure_context: failureContext,
      })
      // 失败通知可能没有正文，只读查询相同轮次补全原因，不重新提交生成。
      const failed = host.checkpoint()
      if (
        !frame[4]?.some((candidate) => candidate?.[1]?.[0]) &&
        failed.response_id &&
        failed.conversation_id
      ) {
        try {
          const history = geminiHistory(session, failed)
          if (history) {
            error = geminiGenerationFailure(frame, {
              ...failureContext,
              message: history.text,
              model: history.actual_model_name,
            })
            host.checkpoint({ ...failed, error, failure_history: history.turn })
          }
        } catch (lookupError) {
          host.checkpoint({ ...failed, failure_lookup_error: String(lookupError.message) })
        }
      }
      throw new Error(error)
    }
    if (frame[1]?.[0] && frame[1]?.[1])
      host.checkpoint({
        ...current,
        phase: 'accepted',
        accepted_at: current.accepted_at || Date.now(),
        conversation_id: frame[1][0],
        response_id: frame[1][1],
        model: model.id,
      })
    const next = geminiParseGeneration(
      JSON.stringify([['wrb.fr', null, JSON.stringify(frame)]]),
      model
    )
    result = {
      ...result,
      ...next,
      text: next.text || result?.text || '',
      actual_model_name: next.actual_model_name || result?.actual_model_name || '',
      reasoning: next.reasoning || result?.reasoning || '',
      reasoning_status: next.reasoning_status || result?.reasoning_status || '',
      conversation_id: next.conversation_id || result?.conversation_id || '',
      response_id: next.response_id || result?.response_id || '',
      candidate_id: next.candidate_id || result?.candidate_id || '',
      token: next.token || result?.token || '',
    }
    host.checkpoint({
      ...host.checkpoint(),
      candidate_id: result.candidate_id,
      token: result.token,
      turn: (previous?.turn || 0) + 1,
      received_text_length: result.text.length,
      actual_model_name: result.actual_model_name,
      received_frame_length: frame.length,
    })
    if (onResult) onResult(result)
  })
  if (!result?.conversation_id || !result.response_id)
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: Gemini 没有返回完整会话编号')
  return result
}

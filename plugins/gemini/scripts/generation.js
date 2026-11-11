function geminiGenerate(session, model, prompt, files, previous, ratio, onResult) {
  const saved = host.checkpoint() || {}
  if (['submitting', 'accepted'].includes(saved.phase))
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 原 Gemini 请求已提交，请恢复原会话')
  let result
  const payload = geminiPayload(prompt, model, files, previous, ratio)
  geminiChannelGenerate(session, model, payload, (frame) => {
    const current = host.checkpoint() || {}
    if (frame[31] === true || frame[31] === 1) {
      const error = 'temporary_unavailable: Gemini 返回生成失败标记 frame[31]'
      host.checkpoint({ ...current, phase: 'failed', error })
      throw new Error(error)
    }
    if (frame[1]?.[0] && frame[1]?.[1])
      host.checkpoint({
        ...current,
        phase: 'accepted',
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
      reasoning: next.reasoning || result?.reasoning || '',
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
      received_frame_length: frame.length,
    })
    if (onResult) onResult(result)
  })
  if (!result?.conversation_id || !result.response_id)
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: Gemini 没有返回完整会话编号')
  return result
}

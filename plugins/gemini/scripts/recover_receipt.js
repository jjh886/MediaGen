// 专用回读入口。此流程没有加载生成和上传脚本。
function main(ctx) {
  const saved = host.checkpoint()
  if (saved.phase === 'failed') throw new Error('receipt_terminal: 上游生成已失败')
  if (
    saved.phase !== 'accepted' ||
    !saved.conversation_id ||
    !saved.response_id ||
    !saved.receipt_writer
  )
    throw new Error('receipt_terminal: 缺少已受理轮次的回执')
  const history = geminiHistory(geminiInitialize(apiSessionRead(ctx)), saved)
  if (!history?.text || history.text.length < (saved.received_text_length || 0))
    throw new Error('receipt_pending: 原轮次尚未取得完整回答')
  const writer = apiTextCreateResponseWriter(saved.receipt_writer.format)
  writer.response_id = saved.receipt_writer.id
  writer.created_at = saved.receipt_writer.created_at
  const reported = history.actual_model_name || saved.actual_model_name
  if (reported)
    writer.request.model =
      'gemini-' +
      reported
        .replace(/\s+(?:low|medium|high)$/i, '')
        .toLowerCase()
        .replace(/[^a-z0-9.]+/g, '-')
        .replace(/^-+|-+$/g, '')
  if (writer.request.session_key)
    writer.metadata = {
      platform_session: {
        conversation_id: saved.conversation_id,
        response_id: saved.response_id,
        candidate_id: history.candidate_id,
        token: saved.token || '',
        turn: saved.turn || 1,
      },
    }
  apiTextFeedResponseText(writer, history.text)
  return { success: true, status: 'done', response: apiTextCompleteResponse(writer) }
}

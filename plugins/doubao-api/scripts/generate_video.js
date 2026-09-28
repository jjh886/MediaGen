function main(ctx) {
  const prompt = String(ctx.prompt || '').trim()
  if (!prompt) throw new Error('missing_input: 缺少视频提示词')
  const saved = host.checkpoint() || {}
  if (saved.phase === 'submitting' && !saved.conversation_id)
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 原视频请求提交状态待核对')
  const session = apiSessionRead(ctx)
  doubaoRequireVerified(session)
  const options = doubaoMediaOptions(ctx, session, 'video', saved.media_options)
  let conversationId = saved.conversation_id
  if (!conversationId) {
    const references = doubaoUploadReferences(ctx, session, options)
    host.checkpoint({ ...saved, phase: 'ready', media_options: options })
    const state = doubaoSubmit(
      session,
      doubaoPayload(prompt, 'video', {
        ...options,
        references: doubaoReferenceBlocks(references),
        reference_message_id: references[0]?.message_id,
      })
    )
    conversationId = state.conversationId
  }
  const creations = doubaoWaitForResult(session, conversationId, 'video')
  const videos = creations.map((item) => {
    const video = item.video
    const url = doubaoVideoDownload(session, video.vid)
    return {
      video_url: url,
      duration: Number(video.duration),
      cover_url: video.cover?.image_ori?.url || video.cover?.image_thumb?.url || '',
      video_id: video.vid,
    }
  })
  doubaoMarkConversationRead(session, conversationId)
  const result = {
    success: true,
    status: 'done',
    source: 'doubao-api',
    model: options.model,
    aspect_ratio: options.ratio,
    videos,
    task_id: conversationId,
    creative_prompt_receipt: { stage: 'doubao_content_text', prompts: [prompt] },
  }
  host.checkpoint({
    ...host.checkpoint(),
    phase: 'completed',
    conversation_id: conversationId,
    result,
  })
  return result
}

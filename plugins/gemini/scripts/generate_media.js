function main(ctx) {
  const kind = ctx.model_id === 'gemini-video' ? 'video' : 'image'
  const prompt = String(ctx.prompt || '').trim()
  if (!prompt) throw new Error('missing_input: 缺少生成提示词')
  let saved = host.checkpoint() || {}
  if (saved.phase === 'completed') return saved.result
  if (saved.phase === 'failed') throw new Error(saved.error)
  const session = geminiInitialize(apiSessionRead(ctx))
  if (!saved.conversation_id) {
    if (saved.phase === 'submitting')
      throw new Error('PROVIDER_SUBMISSION_UNKNOWN: Gemini 原任务提交状态未知')
    const model = geminiModel(session, ctx.model_id, ctx.reasoning_effort)
    host.checkpoint({ ...saved, phase: 'ready' })
    const files = geminiReferences(ctx, session)
    geminiGenerate(
      session,
      model,
      prompt,
      files,
      null,
      ctx.video_aspect_ratio || ctx.image_aspect_ratio
    )
    saved = host.checkpoint()
  }
  const urls = geminiWaitMedia(session, saved, kind)
  const result = {
    success: true,
    status: 'done',
    source: 'gemini',
    ...(kind === 'image'
      ? {
          images: urls.map((url) => ({
            url,
            conversation_id: saved.conversation_id,
            revised_prompt: null,
          })),
          imageCount: urls.length,
        }
      : { videos: urls.map((url) => ({ video_url: url })) }),
  }
  host.checkpoint({ ...saved, phase: 'completed', result })
  return result
}

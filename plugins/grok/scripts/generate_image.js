function main(ctx) {
  const saved = host.checkpoint() || {}
  if (saved.phase === 'completed' && saved.result) return saved.result
  if (ctx.mask_image_dataurl) throw new Error('invalid_input: Grok 不支持局部遮罩')
  if (ctx.background && !['auto', 'opaque'].includes(ctx.background))
    throw new Error('invalid_input: Grok 不支持透明背景')
  const refs = ctx.ref_image_dataurls || []
  if (refs.length > 3) throw new Error('invalid_input: Grok 最多支持三张参考图片')
  const body = {
    model: ctx.model_id,
    prompt: ctx.prompt,
    n: ctx.max_images || 1,
    response_format: 'b64_json',
  }
  if (ctx.quality && ctx.quality !== 'auto') body.quality = ctx.quality
  if (ctx.image_aspect_ratio && ctx.image_aspect_ratio !== 'auto')
    body.aspect_ratio = ctx.image_aspect_ratio
  if (refs.length === 1) body.image = { url: refs[0], type: 'image_url' }
  else if (refs.length) body.images = refs.map((url) => ({ url, type: 'image_url' }))
  host.checkpoint({ phase: 'submitting' })
  const result = grokRequest(
    ctx,
    grokAuth(ctx),
    refs.length ? '/images/edits' : '/images/generations',
    body
  )
  if (!Array.isArray(result.body?.data) || !result.body.data.length)
    throw new Error('Grok 未返回图片')
  const images = result.body.data.map((item) => {
    const base64 = item.b64_json || (item.url && grokAsset(item.url).body_base64)
    if (!base64) throw new Error('Grok 图片内容缺失')
    return {
      url: 'data:image/png;base64,' + base64,
      revised_prompt: item.revised_prompt || ctx.prompt,
    }
  })
  const completed = { success: true, images }
  host.checkpoint({ phase: 'completed', result: completed })
  return completed
}

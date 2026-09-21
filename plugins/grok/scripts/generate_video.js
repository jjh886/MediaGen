function grokVideoURL(value) {
  for (const item of [value, value?.video, value?.output, value?.result]) {
    if (typeof item?.video_url === 'string') return item.video_url
    if (typeof item?.url === 'string') return item.url
  }
  if (Array.isArray(value?.data)) {
    for (const item of value.data) {
      const url = grokVideoURL(item)
      if (url) return url
    }
  } else if (value?.data) return grokVideoURL(value.data)
  return ''
}
function main(ctx) {
  if (ctx.quality && ctx.quality !== 'standard')
    throw new Error('invalid_input: Grok 视频不支持此 quality')
  if (Object.keys(ctx.generation_options || {}).length)
    throw new Error(
      'invalid_input: Grok 视频不支持额外生成参数，请使用 seconds、size 和 input_reference'
    )
  const auth = grokAuth(ctx)
  const refs = ctx.ref_image_dataurls || []
  const seconds = Number(ctx.seconds || 6)
  if (!Number.isInteger(seconds) || seconds < 1 || seconds > 15)
    throw new Error('invalid_input: Grok 视频时长为 1–15 秒')
  if (refs.length > 7) throw new Error('invalid_input: Grok 视频最多支持七张参考图')
  const saved = host.checkpoint() || {}
  let id = saved.upstream_job_id
  if (!id) {
    const resolution =
      Math.min(
        ...String(ctx.size || '1280x720')
          .split('x')
          .map(Number)
      ) >= 1080
        ? '1080p'
        : '720p'
    if (refs.length > 1 && resolution !== '720p')
      throw new Error('invalid_input: Grok 多图视频仅支持 720p')
    const body = {
      model: ctx.model_id,
      prompt: ctx.prompt,
      duration: seconds,
      aspect_ratio: ctx.video_aspect_ratio || '16:9',
      resolution,
    }
    if (refs.length === 1) body.image = { url: refs[0] }
    else if (refs.length) body.reference_images = refs.map((url) => ({ url }))
    host.checkpoint({ phase: 'submitting' })
    const result = grokRequest(ctx, auth, '/videos/generations', body)
    id = result.body?.request_id || result.body?.id || result.body?.data?.request_id
    if (!id || typeof id !== 'string') throw new Error('Grok 视频缺少任务标识')
    host.checkpoint({ phase: 'polling', upstream_job_id: id })
  }
  for (let attempt = 0; attempt < 600; attempt++) {
    const result = grokRequest(ctx, grokAuth(ctx), '/videos/' + encodeURIComponent(id))
    const value =
      result.body?.data && !Array.isArray(result.body.data) ? result.body.data : result.body
    if (
      ['failed', 'error', 'cancelled', 'canceled'].includes(
        String(value?.status || value?.state).toLowerCase()
      ) ||
      value?.error
    )
      throw new Error('Grok 视频生成失败')
    const url = grokVideoURL(result.body)
    if (url) {
      if (!/^https:\/\/(?:[\w.-]+\.)?(?:x\.ai|grok\.com)\//i.test(url))
        throw new Error('Grok 视频地址无效')
      return { success: true, videos: [{ video_url: url }] }
    }
    host.sleep(2000)
  }
  throw new Error('Grok 视频轮询超时')
}

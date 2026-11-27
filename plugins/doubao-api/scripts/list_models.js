function main(ctx) {
  const session = apiSessionRead(ctx)
  const conversation = doubaoLocalConversationId()
  const models = [{ model_id: 'doubao-api-text', display_name: '豆包 文本', task_kind: 'text' }]
  for (const kind of ['image', 'video']) {
    const selector = doubaoMediaSelectors(session, conversation, kind).find(
      (item) => item.key === (kind === 'image' ? 'model' : 'video-model')
    )
    for (const [id, model] of Object.entries(DOUBAO_MEDIA_MODELS)) {
      const option = selector?.option_list?.find((item) => item.option_key === model.value)
      if (model.kind !== kind || !option) continue
      const ref = JSON.parse(option.extra?.ref_media_info || '{}')
      models.push({
        model_id: id.replace('doubao-', 'doubao-api-'),
        display_name: model.value,
        task_kind: kind,
        capabilities: {
          attachment_limits: {
            ...(Number.isSafeInteger(ref.max_ref_video_count)
              ? { max_reference_videos: ref.max_ref_video_count }
              : {}),
            ...(Number.isFinite(ref.max_ref_video_duration)
              ? { max_reference_video_seconds: ref.max_ref_video_duration }
              : {}),
          },
        },
      })
    }
  }
  return { models }
}

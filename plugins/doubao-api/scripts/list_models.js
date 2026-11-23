function main(ctx) {
  const session = apiSessionRead(ctx)
  const conversation = doubaoLocalConversationId()
  const models = [{ model_id: 'doubao-api-text', display_name: '豆包 文本', task_kind: 'text' }]
  for (const kind of ['image', 'video']) {
    const selector = doubaoMediaSelectors(session, conversation, kind).find(
      (item) => item.key === (kind === 'image' ? 'model' : 'video-model')
    )
    for (const [id, model] of Object.entries(DOUBAO_MEDIA_MODELS)) {
      if (
        model.kind === kind &&
        selector?.option_list?.some((opt) => opt.option_key === model.value)
      )
        models.push({
          model_id: id.replace('doubao-', 'doubao-api-'),
          display_name: model.value,
          task_kind: kind,
        })
    }
  }
  return { models }
}

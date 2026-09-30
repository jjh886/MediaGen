const DOUBAO_MEDIA_MODELS = {
  'doubao-seedream-5-pro': { kind: 'image', value: 'Seedream 5.0 Pro' },
  'doubao-seedream-5-flash': { kind: 'image', value: 'Seedream 5.0 Flash' },
  'doubao-seedream-4-5': { kind: 'image', value: 'Seedream 4.5' },
  'doubao-seedream-4-0': { kind: 'image', value: 'Seedream 4.0' },
  'doubao-seedance-2-5': { kind: 'video', value: 'seedance_v2.5' },
  'doubao-seedance-2': { kind: 'video', value: 'seedance_v2.0_std' },
  'doubao-seedance-2-fast': { kind: 'video', value: 'seedance_v2.0' },
  'doubao-seedance-2-mini': { kind: 'video', value: 'seedance_v2.0_mini' },
}

function doubaoMediaOptions(ctx, session, kind, saved) {
  const selected = DOUBAO_MEDIA_MODELS[ctx.model_id]
  if (!selected) throw new Error('invalid_input: 必须指定具体的豆包生成模型')
  if (selected.kind !== kind) throw new Error('invalid_input: 生成类型与模型不一致')
  const requested = ctx.generation_options?.model
  if (requested && requested !== selected.value)
    throw new Error('invalid_input: 模型编号与生成参数中的模型不一致')
  if (saved) {
    if (saved.model !== selected.value)
      throw new Error('invalid_input: 恢复任务的具体模型与原任务不一致')
    return saved
  }
  const conversationId = doubaoLocalConversationId()
  const items = doubaoConversationActions(session, conversationId)
  const entry = items.find((item) => Number(item.instruction_type) === (kind === 'image' ? 4 : 18))
  const conf = session.actionBar?.conf?.item_list?.[entry?.item_id]
  const templates = conf?.instruction_conf?.instruction_item_list
  const selectors = (templates || [])
    .flatMap((item) => JSON.parse(item.template || '{}').selector_list || [])
    .map((item) => JSON.parse(item.template || '{}'))
  const modelSelector = selectors.find(
    (item) => item.key === (kind === 'image' ? 'model' : 'video-model')
  )
  const model = selected.value
  const modelOption = modelSelector?.option_list?.find((item) => item.option_key === model)
  if (!modelOption) throw new Error('account_model_unavailable: 当前账号没有所选豆包模型')
  const configuration = selectors
    .flatMap((item) => Object.values(item.configs || {}))
    .find((item) => item.id === model)
  const options = (configuration?.selectors || []).map((item) => JSON.parse(item.template || '{}'))
  const ratioOption = options.find(
    (item) => item.key === (kind === 'image' ? 'image_creation_ratio' : 'video-ratio')
  )
  if (!ratioOption?.option_list?.length)
    throw new Error('invalid_response: 豆包没有返回模型比例信息')
  const ratio =
    (kind === 'image' ? ctx.image_aspect_ratio : ctx.video_aspect_ratio) ||
    ctx.generation_options?.aspect_ratio ||
    ratioOption.option_list.find((item) => item.id === ratioOption.default_selection_id)?.option_key
  if (!ratioOption.option_list.some((item) => item.option_key === ratio))
    throw new Error('invalid_input: 所选豆包模型不支持该比例')
  const result = { model, ratio, conversationId }
  if (kind === 'video') {
    const durationOption = options.find((item) => item.key === 'video-duration')
    if (!durationOption) throw new Error('invalid_response: 豆包没有返回视频时长信息')
    const duration = Number(
      ctx.seconds ??
        ctx.duration ??
        ctx.video_duration ??
        ctx.generation_options?.duration ??
        durationOption.default_selected_duration
    )
    if (
      !Number.isInteger(duration) ||
      duration < durationOption.lower_bound ||
      duration > durationOption.upper_bound ||
      !(durationOption.step_length > 0) ||
      (duration - durationOption.lower_bound) % durationOption.step_length !== 0
    )
      throw new Error('invalid_input: 所选豆包模型不支持该视频秒数')
    result.duration = duration
    if (modelOption.extra?.ref_media_info)
      result.referenceMedia = JSON.parse(modelOption.extra.ref_media_info)
  }
  return result
}

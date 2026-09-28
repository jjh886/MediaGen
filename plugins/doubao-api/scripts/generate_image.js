function main(ctx) {
  const prompt = String(ctx.prompt || '').trim()
  if (!prompt) throw new Error('missing_input: 缺少图片提示词')
  const count = Number(ctx.max_images || 1)
  if (!Number.isInteger(count) || count < 1 || count > 4)
    throw new Error('invalid_input: 图片数量应为 1 至 4 张')
  const saved = host.checkpoint() || {}
  if (saved.phase === 'submitting' && !saved.conversation_id)
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 原图片请求提交状态待核对')
  const session = apiSessionRead(ctx)
  doubaoRequireVerified(session)
  const options = doubaoMediaOptions(ctx, session, 'image', saved.media_options)
  const images = []
  const histories = {}
  for (const image of saved.images || saved.result?.images || []) {
    if (!histories[image.conversation_id])
      histories[image.conversation_id] = doubaoWaitForResult(
        session,
        image.conversation_id,
        'image'
      )
    const refreshed = histories[image.conversation_id].find(
      (item) => item.image?.key === image.file_id
    )
    if (!refreshed) throw new Error('download_pending: 原图片暂时无法读取，请恢复原任务')
    images.push({ ...image, url: doubaoOriginalImage(refreshed).url })
  }
  let references = saved.references
  let conversationId = saved.phase === 'completed' ? '' : saved.conversation_id
  while (images.length < count) {
    const currentPrompt = prompt
    let creations
    if (!conversationId) {
      references = references || doubaoUploadReferences(ctx, session)
      host.checkpoint({
        ...host.checkpoint(),
        phase: 'ready',
        images,
        references,
        media_options: options,
      })
      const state = doubaoSubmit(
        session,
        doubaoPayload(currentPrompt, 'image', {
          ...options,
          references: doubaoReferenceBlocks(references),
          reference_message_id: references[0]?.message_id,
        })
      )
      conversationId = state.conversationId
      creations = doubaoCreations(state)
    }
    if (!creations?.some((creation) => creation.image?.image_ori_raw?.url))
      creations = doubaoWaitForResult(session, conversationId, 'image')
    const ready = creations.filter((item) => item.image?.image_ori_raw?.url)
    if (!ready.length) throw new Error('invalid_response: 豆包没有返回无水印原图')
    for (const creation of ready.slice(0, count - images.length)) {
      const raw = doubaoOriginalImage(creation)
      images.push({
        url: raw.url,
        width: raw.width,
        height: raw.height,
        file_id: creation.image.key,
        conversation_id: conversationId,
        revised_prompt: null,
        submitted_prompt: currentPrompt,
      })
    }
    doubaoMarkConversationRead(session, conversationId)
    host.checkpoint({
      ...host.checkpoint(),
      phase: 'ready',
      images,
      references,
      media_options: options,
    })
    conversationId = ''
    delete options.conversationId
  }
  const result = {
    success: true,
    status: 'done',
    source: 'doubao-api',
    model: options.model,
    aspect_ratio: options.ratio,
    images,
    imageCount: images.length,
    reference_image_count: references?.length || 0,
    creative_prompt_receipt: {
      stage: 'doubao_content_text',
      prompts: images.map((image) => image.submitted_prompt),
    },
  }
  host.checkpoint({
    ...host.checkpoint(),
    phase: 'completed',
    conversation_id: images[images.length - 1].conversation_id,
    images,
    references,
    result,
  })
  return result
}

function doubaoOriginalImage(creation) {
  const raw = creation.image?.image_ori_raw
  if (!raw?.url || !/^https:\/\//.test(raw.url) || /watermark/i.test(raw.url))
    throw new Error('download_failed: 平台尚未返回无水印原图')
  return raw
}

function main(ctx) {
  const prompt = String(ctx.prompt || '').trim()
  if (!prompt) throw new Error('缺少图片提示词。')

  const saved = chatgptApiPrepareConversationAttempt()
  const auth = chatgptApiRefreshSession(chatgptApiReadAuth(ctx))
  const bootstrap = chatgptApiBootstrap(auth)
  const init = chatgptApiInitializeConversation(auth)
  const generationConfig = chatgptApiGenerationConfig(ctx, bootstrap, init)
  const count = Math.max(1, Math.min(4, Math.floor(Number(ctx.max_images || 1))))
  const aspectRatio = chatgptApiAspectRatio(ctx.image_aspect_ratio)
  let references = chatgptApiReadReferenceFiles(ctx)
  const images = Array.isArray(saved.images)
    ? saved.images.map((image) => {
        if (!image.conversation_id || String(image.url || '').startsWith('data:image/'))
          return image
        const refreshed = chatgptApiWaitForImage(auth, image.conversation_id)
        return { ...image, url: refreshed.url, file_id: refreshed.file_id }
      })
    : []

  for (let index = images.length; index < count; index++) {
    const currentPrompt =
      count > 1 ? prompt + '\n\nCreate variation ' + (index + 1) + ' of ' + count + '.' : prompt
    const turn = chatgptApiGenerateConversation(
      auth,
      currentPrompt,
      aspectRatio,
      references,
      generationConfig,
      init
    )
    references = turn.references
    const result = turn.result
    images.push({
      url: result.url,
      revised_prompt: null,
      submitted_prompt: turn.submitted_prompt,
      file_id: result.file_id,
      conversation_id: result.conversation_id,
      model: turn.model,
      thinking_effort: turn.thinking_effort || null,
    })
    chatgptApiCheckpoint({ phase: 'ready', images, references })
  }

  return {
    success: true,
    status: 'done',
    source: 'chatgpt-api',
    creative_prompt_receipt: images.some((image) => image.submitted_prompt)
      ? {
          stage: 'chatgpt_conversation_text',
          prompts: images.map((image) => image.submitted_prompt).filter(Boolean),
        }
      : undefined,
    reference_image_count: references.length,
    imageCount: images.length,
    images,
  }
}

function chatgptApiGenerationConfig(ctx, bootstrap, init) {
  const legacy = ['ChatGPT', 'chatgpt', 'ChatGPT API', 'chatgpt-api'].includes(ctx.model_id)
  const selected = String(
    legacy
      ? ctx.platform_config?.model || 'chatgpt-image-auto'
      : ctx.model_id || 'chatgpt-image-auto'
  ).trim()
  let model = selected.startsWith('chatgpt-image-')
    ? selected.slice('chatgpt-image-'.length)
    : ['ChatGPT API', 'chatgpt-api'].includes(selected)
      ? 'auto'
      : selected
  if (model === 'auto') {
    model = String(init?.default_model_slug || '').trim()
    if (!model) throw new Error('invalid_response: ChatGPT 未返回默认模型')
  }
  const selection = chatgptApiModelSelection(
    model,
    ctx.reasoning_effort ?? (legacy ? ctx.platform_config?.thinking_effort : undefined),
    bootstrap?.['/backend-api/models']?.models,
    ctx.model_capabilities?.reasoning
  )

  if (model && !/^[a-z0-9][a-z0-9._-]{0,79}$/i.test(model)) {
    throw new Error('ChatGPT 模型标识格式不正确，请在生成配置中检查。')
  }

  return selection
}

function chatgptApiAspectRatio(value) {
  const normalized = String(value || 'auto').trim()
  return ['auto', '9:16', '16:9', '1:1', '3:4', '4:3', '2:1', '3:2', '2:3'].includes(normalized)
    ? normalized
    : 'auto'
}

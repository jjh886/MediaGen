function main(ctx) {
  const prompt = String(ctx.prompt || '').trim()
  if (!prompt) throw new Error('缺少图片提示词。')

  const auth = chatgptApiRefreshSession(chatgptApiReadAuth(ctx))
  const bootstrap = chatgptApiBootstrap(auth)
  const generationConfig = chatgptApiGenerationConfig(ctx, bootstrap)
  const count = Math.max(1, Math.min(4, Math.floor(Number(ctx.max_images || 1))))
  const aspectRatio = chatgptApiAspectRatio(ctx.image_aspect_ratio)
  let references = chatgptApiReadReferenceFiles(ctx)
  const images = []

  for (let index = 0; index < count; index++) {
    chatgptApiCheckpoint({ phase: 'ready' })
    const currentPrompt =
      count > 1 ? prompt + '\n\nCreate variation ' + (index + 1) + ' of ' + count + '.' : prompt
    const turn = chatgptApiGenerateConversation(
      auth,
      currentPrompt,
      aspectRatio,
      references,
      generationConfig
    )
    references = turn.references
    const result = turn.result
    images.push({
      url: result.url,
      revised_prompt: null,
      file_id: result.file_id,
      conversation_id: result.conversation_id,
      model: turn.model,
      thinking_effort: turn.thinking_effort || null,
    })
  }

  return {
    success: true,
    status: 'done',
    source: 'chatgpt-api',
    reference_image_count: references.length,
    imageCount: images.length,
    images,
  }
}

function chatgptApiGenerationConfig(ctx, bootstrap) {
  const raw = ctx?.platform_config
  const config = raw && typeof raw === 'object' && !Array.isArray(raw) ? raw : {}
  const model = String(config.model || '').trim()
  const thinkingEffort = String(
    config.thinking_effort === undefined ? 'extended' : config.thinking_effort
  ).trim()

  if (model && !/^[a-z0-9][a-z0-9._-]{0,79}$/i.test(model)) {
    throw new Error('ChatGPT API 模型标识格式不正确，请在生成配置中检查。')
  }
  if (!['', 'standard', 'extended'].includes(thinkingEffort)) {
    throw new Error('ChatGPT API 思考深度无效，只能选择跟随默认、Standard 或 Extended。')
  }

  const availableModels = chatgptApiAvailableModels(bootstrap)
  if (model && availableModels.length && !availableModels.includes(model)) {
    throw new Error(
      '当前 ChatGPT 账号不可用模型 ' +
        model +
        '，可用模型：' +
        availableModels.slice(0, 12).join('、')
    )
  }
  return { model, thinking_effort: thinkingEffort }
}

function chatgptApiAvailableModels(bootstrap) {
  const models = bootstrap?.['/backend-api/models']?.models
  if (!Array.isArray(models)) return []
  // 模型权限由当前账号实时返回，不能在插件里写死会过期的模型名单。
  return models
    .map((model) => String(model?.slug || '').trim())
    .filter((model, index, values) => model && values.indexOf(model) === index)
}

function chatgptApiAspectRatio(value) {
  const normalized = String(value || 'auto').trim()
  return ['auto', '9:16', '16:9', '1:1', '3:4', '4:3', '2:1'].includes(normalized)
    ? normalized
    : 'auto'
}

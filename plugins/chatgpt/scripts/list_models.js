function main(ctx) {
  const auth = chatgptApiRefreshSession(chatgptApiReadAuth(ctx))
  const result = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'GET', CHATGPT_MODELS_PATH, {
      target_path: '/backend-api/models',
      target_route: '/backend-api/models',
    }),
    '获取模型'
  )
  if (!Array.isArray(result.body?.models)) throw new Error('模型目录响应无效')
  const catalog = result.body
  const entries = []
  const unavailablePro = new Set()
  for (const family of CHATGPT_CHAT_MODELS) {
    const routes = chatgptApiPickerOptions(catalog, family)
    if (!routes.has('pro')) unavailablePro.add(family.base + '-pro')
    const base = catalog.models.find((m) => m?.slug === family.base)
    if (!routes.size && (!base || Array.isArray(catalog.versions))) continue
    entries.push({
      model_id: family.id,
      display_name: family.title,
      aliases: base && family.id === 'chatgpt-' + base.slug ? [base.slug] : [],
      max_tokens: base?.max_tokens,
      reasoning: {
        efforts: [...routes.keys()],
        default_effort: routes.has('standard') ? 'standard' : undefined,
      },
    })
  }
  // 旧调用和图片模型依赖仍可按账号的实际型号解析；对话目录只列网页家族。
  for (const item of catalog.models) {
    if (
      typeof item?.slug !== 'string' ||
      item.slug.endsWith('-wm') ||
      unavailablePro.has(item.slug) ||
      CHATGPT_CHAT_MODELS.some((f) => f.id === 'chatgpt-' + item.slug)
    )
      continue
    const version = /^gpt-(\d+)(?:[.-](\d+))?(?:$|-)/.exec(item.slug)
    if (
      !version ||
      !(Number(version[1]) > 5 || (Number(version[1]) === 5 && Number(version[2]) >= 6))
    )
      continue
    entries.push({
      model_id: 'chatgpt-' + item.slug,
      display_name: item.title || item.slug,
      aliases: [item.slug],
      catalog_hidden: true,
      max_tokens: item.max_tokens,
      reasoning: chatgptApiDiscoveredReasoning(item, catalog.models),
    })
  }
  return {
    models: entries.map(({ max_tokens, reasoning, ...entry }) => ({
      ...entry,
      task_kind: 'text',
      capabilities: {
        input_schema: {
          type: 'object',
          required: ['model', 'input'],
          properties: {
            model: { type: 'string' },
            input: { oneOf: [{ type: 'string' }, { type: 'array' }] },
            stream: { type: 'boolean' },
            instructions: { type: 'string' },
            reasoning: { type: 'object' },
          },
        },
        input_modalities: ['text', 'image', 'video', 'audio', 'file'],
        attachment_transport: 'workspace',
        attachment_limits: { file_bytes: 536870912, image_bytes: 20971520 },
        file_extensions: [
          'pdf',
          'txt',
          'md',
          'csv',
          'json',
          'doc',
          'docx',
          'xls',
          'xlsx',
          'ppt',
          'pptx',
          'py',
          'js',
          'jsx',
          'ts',
          'tsx',
          'go',
          'rs',
          'java',
          'c',
          'cpp',
          'h',
          'cs',
          'php',
          'rb',
          'sh',
          'sql',
          'html',
          'css',
          'xml',
          'yaml',
          'yml',
          'toml',
          'log',
          'tsv',
          'rtf',
          'odt',
          'eml',
          'ods',
          'odp',
        ],
        image_sources: ['data_url', 'file_id'],
        output_modalities: ['text', 'image'],
        endpoints: ['/v1/responses', '/v1/chat/completions'],
        streaming: true,
        tool_calling: false,
        unsupported_parameters: [
          'temperature',
          'top_p',
          'presence_penalty',
          'frequency_penalty',
          'max_output_tokens',
          'stop',
          'logprobs',
          'top_logprobs',
          'logit_bias',
          'seed',
          'prediction',
          'audio',
          'modalities',
          'text',
          'prompt',
          'conversation',
          'context_management',
          'prompt_cache_key',
          'prompt_cache_retention',
          'max_tool_calls',
          'include',
          'service_tier',
          'truncation',
          'safety_identifier',
        ],
        context_window: max_tokens,
        reasoning,
      },
    })),
  }
}

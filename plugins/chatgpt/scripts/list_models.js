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
  for (const family of CHATGPT_CHAT_MODELS) {
    const routes = chatgptApiPickerOptions(catalog, family)
    const base = catalog.models.find((m) => m?.slug === family.base)
    if (!routes.size && (!base || Array.isArray(catalog.versions))) continue
    entries.push({
      model_id: family.id,
      display_name: family.title,
      max_tokens: base?.max_tokens,
      reasoning: {
        efforts: [...routes.keys()],
        default_effort: routes.has('standard') ? 'standard' : undefined,
      },
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

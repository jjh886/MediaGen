function main(ctx) {
  const auth = chatgptApiRefreshSession(chatgptApiReadAuth(ctx))
  const result = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'GET', '/backend-api/models'),
    '获取模型'
  )
  if (!Array.isArray(result.body?.models)) throw new Error('模型目录响应无效')
  return {
    models: result.body.models.map((item) => ({
      model_id: item.slug,
      display_name: item.title || item.slug,
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
        input_modalities: ['text', 'image'],
        image_sources: ['data_url', 'file_id'],
        output_modalities: ['text'],
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
        ],
        context_window: item.max_tokens,
        reasoning: {
          default_effort: item.default_thinking_effort,
          efforts: (item.thinking_efforts || []).map((level) =>
            typeof level === 'string' ? level : level.thinking_effort
          ),
        },
      },
    })),
  }
}

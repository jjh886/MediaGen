function main(ctx) {
  const auth = codexEnsureFreshAuth(ctx)
  const result = host.call('http.request', {
    method: 'GET',
    url: 'https://chatgpt.com/backend-api/codex/models?client_version=' + CODEX_UPSTREAM_VERSION,
    headers: codexUpstreamHeaders(
      codexAccessToken(auth),
      codexAccountId(auth),
      'application/json',
      ctx
    ),
    fingerprint: 'none',
    timeout_ms: 30000,
  })
  if (!result.ok || !Array.isArray(result.body?.models))
    throw new Error('模型目录获取失败: HTTP ' + result.status)
  return {
    models: result.body.models
      .filter((item) => typeof item?.slug === 'string' && codexSupportedTextModel(item.slug))
      .map((item) => ({
        model_id: item.slug,
        display_name: item.display_name || item.slug,
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
          input_modalities: [...new Set([...(item.input_modalities || ['text']), 'file'])],
          output_modalities: ['text'],
          endpoints: ['/v1/responses', '/v1/chat/completions', '/v1/responses/compact'],
          streaming: true,
          tool_calling: true,
          transports: ['http', 'websocket'],
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
          ],
          context_window: item.context_window,
          max_context_window: item.max_context_window,
          reasoning: {
            efforts: (item.supported_reasoning_levels || [])
              .map((level) => level.effort)
              .filter((effort) => codexSupportedEffort(effort, item.slug)),
            default_effort: codexSupportedEffort(item.default_reasoning_level, item.slug)
              ? item.default_reasoning_level
              : undefined,
          },
        },
      })),
  }
}

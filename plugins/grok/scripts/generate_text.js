function main(ctx) {
  const saved = host.checkpoint() || {}
  if (saved.phase === 'completed' && saved.result) return saved.result
  const result = { success: true, response: grokText(ctx, grokAuth(ctx), ctx.request) }
  if (ctx.request.options?.session_key || ctx.request.prompt_cache_key)
    result.response.metadata = {
      ...result.response.metadata,
      platform_session: {
        session_id: ctx.request.options?.session_key || ctx.request.prompt_cache_key,
      },
    }
  host.checkpoint({ phase: 'completed', result })
  return result
}

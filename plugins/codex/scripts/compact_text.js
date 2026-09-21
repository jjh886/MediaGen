function main(ctx) {
  const normalized = codexNormalizeResponsesRequest(ctx.request, ctx.platform_config)
  normalized.payload.input.push({ type: 'compaction_trigger' })
  const auth = codexEnsureFreshAuth(ctx)
  codexApplyReasoning(ctx, auth, normalized.payload)
  const result = codexExecuteResponses(auth, normalized.payload)
  if (result.error || !result.response) throw new Error(result.error || '上游压缩失败')
  if (!result.response.output?.some((item) => item.type === 'compaction'))
    throw new Error('unsupported_compaction: 当前账号没有返回压缩上下文')
  return {
    success: true,
    response: {
      id: result.response.id,
      object: 'response.compaction',
      created_at: result.response.created_at,
      output: result.response.output,
      usage: result.response.usage,
    },
  }
}

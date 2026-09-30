function main(ctx) {
  const normalized = codexNormalizeResponsesRequest(ctx.request, ctx.platform_config)
  let auth = codexEnsureFreshAuth(ctx)
  codexApplyReasoning(ctx, auth, normalized.payload)
  host.checkpoint({ phase: 'submitting' })
  let execution = codexExecuteResponses(auth, normalized.payload, ctx)
  if (execution.status >= 400 && execution.status < 500) {
    host.checkpoint({ phase: 'ready' })
  }
  if (!execution.response && execution.status === 401) {
    auth = codexRefreshAuth(ctx, auth, '文本接口返回 401')
    host.checkpoint({ phase: 'submitting' })
    execution = codexExecuteResponses(auth, normalized.payload, ctx)
    if (execution.status >= 400 && execution.status < 500) {
      host.checkpoint({ phase: 'ready' })
    }
  }
  if (execution.error || !execution.response) {
    throw new Error(execution.error || 'temporary_unavailable: Codex Responses 缺少完成事件')
  }
  return {
    success: true,
    status: 'done',
    source: 'codex',
    response: execution.response,
  }
}

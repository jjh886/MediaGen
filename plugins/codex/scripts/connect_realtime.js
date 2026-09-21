function main(ctx) {
  const auth = codexEnsureFreshAuth(ctx)
  const model = String(ctx.request.model || 'gpt-realtime-2.1')
  if (model !== 'gpt-realtime-2.1')
    throw new Error('unsupported_model: 此模型不支持官方实时 WebSocket')
  return {
    connection: {
      url: 'wss://api.openai.com/v1/realtime?model=' + encodeURIComponent(model),
      headers: { authorization: 'Bearer ' + codexAccessToken(auth) },
      fingerprint: 'none',
    },
  }
}

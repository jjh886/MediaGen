function main(ctx) {
  const saved = host.checkpoint() || {}
  if (saved.phase === 'completed' && saved.result) return saved.result
  const request = ctx.request
  const allowed = ['model', 'file', 'language', 'response_format']
  for (const key of Object.keys(request))
    if (!allowed.includes(key)) throw new Error('invalid_input: Grok 转写不支持 ' + key)
  if (!['json', 'text'].includes(request.response_format || 'json'))
    throw new Error('invalid_input: Grok 转写支持 json 或 text 格式')
  const body = { file: request.file, model: 'grok-voice-transcribe-2.0' }
  if (request.language) body.language = request.language
  const auth = grokAuth(ctx)
  const headers = grokHeaders(ctx, auth)
  delete headers['content-type']
  host.checkpoint({ phase: 'submitting' })
  const result = grokRequest(ctx, auth, '/stt', body, { body_format: 'multipart', headers })
  if (typeof result.body?.text !== 'string') throw new Error('Grok 未返回有效转写文本')
  const completed = { success: true, text: result.body.text }
  host.checkpoint({ phase: 'completed', result: completed })
  return completed
}

function main(ctx) {
  const request = ctx.request
  if (request.instructions) throw new Error('invalid_input: Grok TTS 不支持独立 instructions 参数')
  if (request.speed !== undefined && (request.speed < 0.7 || request.speed > 1.5))
    throw new Error('invalid_input: Grok speed 必须在 0.7 到 1.5 之间')
  const format = request.response_format || 'mp3'
  host.checkpoint({ phase: 'submitting' })
  const result = grokRequest(
    ctx,
    grokAuth(ctx),
    '/tts',
    {
      text: request.input,
      voice_id: request.voice || 'eve',
      language: 'auto',
      speed: request.speed === undefined ? 1 : request.speed,
      output_format: { codec: 'mp3', sample_rate: 24000, bit_rate: 128000 },
    },
    { response_body_format: 'base64' }
  )
  if (!result.body_base64) throw new Error('Grok 未返回语音数据')
  const audio =
    format === 'mp3'
      ? { base64: result.body_base64, mime_type: 'audio/mpeg' }
      : host.call('audio.transcode', { base64: result.body_base64, format })
  return { success: true, audio_base64: audio.base64, mime_type: audio.mime_type, format }
}

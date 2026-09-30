function main(ctx) {
  const request = ctx.request
  if (!request || typeof request !== 'object' || Array.isArray(request))
    throw new Error('invalid_input: 语音请求必须是对象')
  if (typeof request.input !== 'string' || !request.input.trim())
    throw new Error('missing_input: 语音请求缺少 input')
  if (request.input.length > 100000) throw new Error('invalid_input: 语音输入超过长度限制')
  if (request.speed !== undefined && request.speed !== 1)
    throw new Error('invalid_input: 当前语音模型仅支持 speed=1')
  if (request.instructions) throw new Error('invalid_input: 当前语音模型不支持 instructions')
  const auth = chatgptApiRefreshSession(chatgptApiReadAuth(ctx))
  const format = request.response_format || 'mp3'
  const transcode = ['wav', 'pcm', 'flac'].includes(format)
  const result = chatgptApiGenerateSpeech(
    auth,
    request.input,
    request.voice,
    transcode ? 'aac' : format
  )
  if (transcode) {
    const converted = host.call('audio.transcode', { base64: result.audio_base64, format })
    result.audio_base64 = converted.base64
    result.mime_type = converted.mime_type
    result.format = format
  }
  return { success: true, ...result }
}

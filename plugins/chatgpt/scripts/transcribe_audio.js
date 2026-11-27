function chatgptApiTranscribe(ctx) {
  const request = ctx.request
  for (const key of [
    'prompt',
    'temperature',
    'timestamp_granularities',
    'stream',
    'include',
    'chunking_strategy',
  ]) {
    if (request[key] !== undefined && request[key] !== false)
      throw new Error('invalid_input: 当前转写模型不支持 ' + key)
  }
  if (!['json', 'text'].includes(request.response_format || 'json'))
    throw new Error('invalid_input: 当前转写模型仅支持 json 和 text')
  const auth = chatgptApiRefreshSession(chatgptApiReadAuth(ctx))
  const body = request.file.file_path ? {} : { file: request.file }
  if (request.language) body.language = request.language
  const response = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/transcribe', {
      ...(request.file.file_path
        ? {
            body_format: 'multipart_file',
            body_file: request.file.file_path,
            body_file_name: request.file.filename,
            body_file_mime: request.file.mime_type,
            body_file_size: request.file.size,
          }
        : { body_format: 'multipart' }),
      body,
      timeout_ms: 300000,
    }),
    '音频转写'
  )
  const text = typeof response.body === 'string' ? response.body : response.body?.text
  if (typeof text !== 'string') throw new Error('音频转写结果无效')
  return { success: true, text }
}

function main(ctx) {
  return chatgptApiTranscribe(ctx)
}

function geminiUpload(session, attachment) {
  const limit = attachment.mime_type.startsWith('video/') ? 2 * 1024 ** 3 : 100 * 1024 ** 2
  if (attachment.size > limit)
    throw new Error('invalid_input: Gemini 附件超过上游单文件大小限制（视频2GB，其他文件100MB）')
  if (!session.tokens.push_id) throw new Error('upstream_client_outdated: Gemini 页面缺少上传参数')
  const headers = {
    'x-tenant-id': 'bard-storage',
    'push-id': session.tokens.push_id,
    'content-type': 'application/x-www-form-urlencoded;charset=UTF-8',
    ...(session.tokens.pctx ? { 'x-client-pctx': session.tokens.pctx } : {}),
  }
  const start = geminiRequest(
    session,
    'https://content-push.googleapis.com/upload?authuser=' + (session.auth.authuser || 0),
    {
      method: 'POST',
      headers: {
        ...headers,
        'x-goog-upload-command': 'start',
        'x-goog-upload-protocol': 'resumable',
        'x-goog-upload-header-content-length': String(attachment.size),
      },
      body_format: 'text',
      body: 'File name: ' + attachment.name.replace(/[\r\n]/g, ''),
    }
  )
  const url = start.headers?.['x-goog-upload-url']
  if (!/^https:\/\/content-push\.googleapis\.com\/upload(?:[/?]|$)/.test(url || ''))
    throw new Error('upload_failed: Gemini 返回了无效上传地址')
  const result = geminiRequest(session, url, {
    method: 'POST',
    headers: {
      ...headers,
      'x-goog-upload-command': 'upload, finalize',
      'x-goog-upload-offset': '0',
    },
    ...(attachment.path
      ? { body_format: 'file', body_file: attachment.path, body_file_size: attachment.size }
      : { body_format: 'base64', body: attachment.base64 }),
    timeout_ms: 900000,
  })
  const reference = String(result.body_text || '').trim()
  if (!/^\/[^\s\r\n]+$/.test(reference)) throw new Error('upload_failed: Gemini 未返回文件引用')
  const kind = attachment.mime_type.startsWith('image/')
    ? 1
    : attachment.mime_type.startsWith('video/')
      ? 2
      : 3
  return [
    [reference, kind, null, attachment.mime_type],
    attachment.name,
    null,
    null,
    null,
    null,
    null,
    null,
    [0],
  ]
}

function geminiReferences(ctx, session, request) {
  const attachments = request
    ? request.input.flatMap((item) => item.attachments || [])
    : apiTextResponsesMedia(
        (ctx.ref_image_dataurls || []).map((url) => ({ type: 'input_image', image_url: url }))
      )
  if (!request) {
    for (const [paths, type] of [
      [ctx.ref_image_locals || [], 'image/png'],
      [ctx.ref_video_locals || [], 'video/mp4'],
      [ctx.ref_audio_locals || [], 'audio/mpeg'],
    ]) {
      for (const path of paths) {
        const file = host.call(
          type.startsWith('image/') ? 'file.inspect_image' : 'file.inspect_media',
          { path, max_bytes: type.startsWith('video/') ? 2 * 1024 ** 3 : 100 * 1024 ** 2 }
        )
        const name = path.split(/[\\/]/).pop()
        const extension = name.split('.').pop().toLowerCase()
        attachments.push({
          name,
          path,
          size: file.size,
          mime_type: API_TEXT_FILE_TYPES[extension] || type,
        })
      }
    }
  }
  if (attachments.length > 10) throw new Error('invalid_input: Gemini 同一提示最多10个附件')
  return attachments.map((attachment, index) =>
    geminiUpload(
      session,
      request ? apiTextMaterializeAttachment(ctx, attachment, index) : attachment
    )
  )
}

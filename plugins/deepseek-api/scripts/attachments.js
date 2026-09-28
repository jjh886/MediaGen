function deepseekValidateAttachments(request) {
  const attachments = request.input.flatMap((item) => item.attachments || [])
  if (attachments.length > 50) throw new Error('invalid_input: DeepSeek 单次附件不能超过 50 个')
  if (attachments.some((attachment) => /^(video|audio)\//.test(attachment.mime_type)))
    throw new Error('invalid_input: 当前 DeepSeek 网页不支持音视频附件')
  return attachments
}

function deepseekUploadAttachments(ctx, session, request) {
  const attachments = deepseekValidateAttachments(request)
  const ids = []
  for (let index = 0; index < attachments.length; index++) {
    const attachment = attachments[index]
    const file = apiTextMaterializeAttachment(ctx, attachment, index)
    const pow = deepseekPow(session, '/api/v0/file/upload_file')
    const headers = deepseekHeaders(session, '/api/v0/file/upload_file')
    delete headers['content-type']
    const response = apiSessionRequireOk(
      host.call('http.request', {
        method: 'POST',
        url: DEEPSEEK_ORIGIN + '/api/v0/file/upload_file',
        headers: {
          ...headers,
          'x-ds-pow-response': pow,
          'x-file-size': String(file.size),
          'x-model-type': 'default',
          'x-thinking-enabled': request.reasoning.effort === 'none' ? '0' : '1',
        },
        body_format: 'multipart_file',
        body_file: file.path,
        body_file_field: 'file',
        body_file_name: file.name,
        body_file_mime: file.mime_type,
        body_file_size: file.size,
        proxy: 'system',
        timeout_ms: 180000,
        retry: { max_attempts: 1 },
      }),
      'DeepSeek 附件上传失败'
    )
    deepseekRefreshSession(session, response, '/api/v0/file/upload_file')
    const uploaded = response.body
    if (uploaded?.code !== 0 || uploaded.data?.biz_code !== 0 || !uploaded.data.biz_data?.id) {
      throw new Error('upload_failed: DeepSeek 拒绝了附件')
    }
    ids.push(uploaded.data.biz_data.id)
  }
  if (!ids.length) return ids
  const deadline = Date.now() + 10 * 60 * 1000
  while (Date.now() < deadline) {
    const data = deepseekRequest(
      session,
      '/file/fetch_files?' + ids.map((id) => 'file_ids=' + encodeURIComponent(id)).join('&'),
      undefined,
      'GET'
    )
    const files = data.files || []
    if (files.some((file) => ['FAILED', 'ERROR'].includes(file.status) || file.error_code)) {
      throw new Error('attachment_rejected: DeepSeek 无法解析附件')
    }
    if (ids.every((id) => files.some((file) => file.id === id && file.status === 'SUCCESS')))
      return ids
    host.sleep(1000)
  }
  throw new Error('attachment_timeout: DeepSeek 附件解析超时')
}

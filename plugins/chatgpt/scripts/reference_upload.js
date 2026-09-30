function chatgptApiReadReferenceFiles(ctx) {
  const paths = chatgptApiStringArray(ctx.ref_image_locals)
  if (!paths.length && chatgptApiStringArray(ctx.ref_image_dataurls).length) {
    throw new Error('参考图尚未在本机准备完成，无法上传。')
  }
  return paths.map((path, index) => {
    const dimensions = host.call('file.inspect_image', {
      path,
      max_bytes: 50 * 1024 * 1024,
    })
    const extension = chatgptApiImageExtension(dimensions.mime)
    return {
      path,
      mime_type: dimensions.mime,
      name: 'mediagen-reference-' + (index + 1) + '.' + extension,
      size: Number(dimensions.size || 0),
      width: Number(dimensions.width || 0),
      height: Number(dimensions.height || 0),
    }
  })
}

function chatgptApiUploadReferences(auth, references) {
  if (references?.length) chatgptApiReserveUpload(auth)
  return (references || []).map((reference) => chatgptApiUploadReference(auth, reference))
}

function chatgptApiReserveUpload(auth) {
  const path = '/backend-api/files/upload_reservations'
  chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', path, {
      target_path: path,
      target_route: path,
      headers: { 'content-type': 'application/json' },
      body: {
        intended_use_case: 'my_files',
        entry_surface: 'chat_composer',
        requires_gizmo_id: false,
        store_in_library: true,
        library_persistence_mode: 'opportunistic',
      },
      timeout_ms: 60000,
    }),
    '预留 ChatGPT 附件上传'
  )
}

function chatgptApiUploadReference(auth, reference) {
  const fingerprint = chatgptApiFingerprint(auth)
  const useCase = reference.use_case || 'multimodal'
  const created = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/files', {
      target_path: '/backend-api/files',
      target_route: '/backend-api/files',
      headers: { 'content-type': 'application/json' },
      body: {
        file_name: reference.name,
        file_size: reference.size,
        use_case: useCase,
        timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
        reset_rate_limits: false,
        supports_direct_azure_multipart: true,
        mime_type: reference.mime_type,
        entry_surface: 'chat_composer',
        ...(useCase === 'multimodal' ? { selection_method: 'clipboard_file' } : {}),
        client_resolved_mime_type: reference.mime_type,
        mime_resolution_source: 'filename_extension',
        store_in_library: true,
        library_persistence_mode: 'opportunistic',
      },
      timeout_ms: 60000,
    }),
    '创建 ChatGPT 参考图文件'
  ).body
  if (!created?.file_id || !created?.upload_url) {
    throw new Error('创建 ChatGPT 参考图文件后缺少 file_id 或 upload_url')
  }

  chatgptApiUploadSignedBlob(auth, created.upload_url, reference)
  const processed = chatgptApiProcessReference(auth, created.file_id, reference)
  if (useCase === 'multimodal') chatgptApiVerifyReference(auth, created.file_id)
  return {
    file_id: created.file_id,
    library_file_id: processed.library_file_id,
    name: processed.name || reference.name,
    mime_type: processed.mime_type || reference.mime_type,
    size: reference.size,
    width: reference.width,
    height: reference.height,
  }
}

function chatgptApiUploadSignedBlob(auth, uploadUrl, reference) {
  const response = host.call('http.request', {
    method: 'PUT',
    url: uploadUrl,
    proxy: 'system',
    tls_profile: chatgptApiTlsProfile(auth),
    headers: {
      'content-type': reference.mime_type,
      'x-ms-blob-type': 'BlockBlob',
    },
    body_file: reference.path,
    body_file_size: reference.size,
    body_format: 'file',
    timeout_ms: 180000,
  })
  chatgptApiRequireOk(response, '上传 ChatGPT 参考图二进制')
}

function chatgptApiProcessReference(auth, fileId, reference) {
  const response = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/files/process_upload_stream', {
      target_path: '/backend-api/files/process_upload_stream',
      target_route: '/backend-api/files/process_upload_stream',
      headers: { 'content-type': 'application/json' },
      body: {
        file_id: fileId,
        use_case: reference.use_case || 'multimodal',
        index_for_retrieval: reference.use_case === 'my_files',
        file_name: reference.name,
        mime_type: reference.mime_type,
        library_persistence_mode: 'opportunistic',
        entry_surface: 'chat_composer',
        metadata: {
          store_in_library: true,
          is_temporary_chat: false,
          library_eligibility_reason: 'eligible',
          is_project_thread: false,
        },
      },
      timeout_ms: 180000,
    }),
    '处理 ChatGPT 参考图'
  )
  const events = String(response.body_text || '')
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => JSON.parse(line))
  if (!events.some((event) => event?.event === 'file.processing.completed')) {
    throw new Error('ChatGPT 参考图处理流没有返回 file.processing.completed')
  }
  const indexed =
    events.find((event) => event?.event === 'file.indexing.completed') ||
    events.find((event) => event?.event === 'file.processing.completed')
  return {
    library_file_id: indexed?.extra?.metadata_object_id || '',
    name: indexed?.extra?.library_file_name || reference.name,
    mime_type: indexed?.extra?.mime_type || reference.mime_type,
  }
}

function chatgptApiUploadTextAttachments(ctx, auth, input) {
  chatgptApiTranscribeAttachments(auth, input)
  const attachments = input.flatMap((item) => item.attachments || [])
  if (!attachments.length) return []
  if (typeof ctx.workspace_dir !== 'string' || !ctx.workspace_dir) {
    throw new Error('invalid_input: 附件上传工作目录缺失。')
  }
  chatgptApiReserveUpload(auth)
  return attachments.map((attachment, index) => {
    const file = apiTextMaterializeAttachment(ctx, attachment, index)
    const image = attachment.mime_type.startsWith('image/')
    const dimensions = image ? host.call('file.inspect_image', { path: file.path }) : {}
    return chatgptApiUploadReference(auth, {
      path: file.path,
      name: attachment.name,
      mime_type: attachment.mime_type,
      size: file.size,
      ...(image ? { width: dimensions.width, height: dimensions.height } : {}),
      use_case: image
        ? 'multimodal'
        : /^(audio|video)\//.test(attachment.mime_type)
          ? 'ace_upload'
          : 'my_files',
    })
  })
}

function chatgptApiTranscribeAttachments(auth, input) {
  for (const item of input) {
    const retained = []
    for (const attachment of item.attachments || []) {
      if (!attachment.mime_type.startsWith('audio/')) {
        retained.push(attachment)
        continue
      }
      // 普通文件上传不会把音频交给听觉模型；复用平台已有的语音转写入口。
      const response = chatgptApiRequireOk(
        chatgptApiRequest(auth, 'POST', '/backend-api/transcribe', {
          body_format: 'multipart',
          body: {
            file: {
              base64: attachment.base64,
              filename: attachment.name,
              mime_type: attachment.mime_type,
            },
          },
          timeout_ms: 300000,
        }),
        '转写 ChatGPT 音频附件'
      )
      const text = typeof response.body === 'string' ? response.body : response.body?.text
      if (typeof text !== 'string' || !text.trim())
        throw new Error('invalid_response: ChatGPT 音频附件未返回有效转写')
      item.text =
        (item.text || '') +
        '\n\n音频附件 ' +
        attachment.name +
        ' 的语音转写（仅含识别的语音，不代表已分析音色、音乐或其他声音）：\n' +
        text
    }
    item.attachments = retained
  }
}

function chatgptApiVerifyReference(auth, fileId) {
  const encoded = encodeURIComponent(fileId)
  chatgptApiRequireOk(
    chatgptApiRequest(auth, 'GET', '/backend-api/files/' + encoded + '/simple', {
      target_path: '/backend-api/files/' + encoded + '/simple',
      target_route: '/backend-api/files/{file_id}/simple',
      timeout_ms: 60000,
    }),
    '确认 ChatGPT 参考图可用'
  )
  chatgptApiRequireOk(
    chatgptApiRequest(
      auth,
      'GET',
      '/backend-api/files/download/' + encoded + '?download_intent=false',
      {
        target_path: '/backend-api/files/download/' + encoded,
        target_route: '/backend-api/files/download/{file_id}',
        timeout_ms: 60000,
      }
    ),
    '确认 ChatGPT 参考图下载状态'
  )
}

function chatgptApiStringArray(value) {
  return Array.isArray(value) ? value.map((item) => String(item || '').trim()).filter(Boolean) : []
}

function chatgptApiImageExtension(mime) {
  if (mime === 'image/jpeg') return 'jpg'
  if (mime === 'image/webp') return 'webp'
  if (mime === 'image/gif') return 'gif'
  return 'png'
}

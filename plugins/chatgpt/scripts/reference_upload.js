function chatgptApiReadReferenceFiles(ctx) {
  const paths = chatgptApiStringArray(ctx.ref_image_locals)
  if (!paths.length && chatgptApiStringArray(ctx.ref_image_dataurls).length) {
    throw new Error('参考图没有被服务端转换为本地临时文件，无法安全上传。')
  }
  return paths.map((path, index) => {
    const file = host.call('file.read_base64', { path, max_bytes: 50 * 1024 * 1024 })
    const dimensions = host.call('file.inspect_image', {
      path,
      max_bytes: 50 * 1024 * 1024,
    })
    const extension = chatgptApiImageExtension(file.mime)
    return {
      base64: file.base64,
      mime_type: file.mime,
      name: 'mediagen-reference-' + (index + 1) + '.' + extension,
      size: Number(file.size || 0),
      width: Number(dimensions.width || 0),
      height: Number(dimensions.height || 0),
    }
  })
}

function chatgptApiUploadReferences(auth, references) {
  return (references || []).map((reference) => chatgptApiUploadReference(auth, reference))
}

function chatgptApiUploadReference(auth, reference) {
  const fingerprint = chatgptApiFingerprint(auth)
  const created = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/files', {
      target_path: '/backend-api/files',
      target_route: '/backend-api/files',
      headers: { 'content-type': 'application/json' },
      body: {
        file_name: reference.name,
        file_size: reference.size,
        use_case: 'multimodal',
        timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
        reset_rate_limits: false,
        supports_direct_azure_multipart: true,
        mime_type: reference.mime_type,
        entry_surface: 'chat_composer',
        selection_method: 'clipboard_file',
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
  chatgptApiVerifyReference(auth, created.file_id)
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
      ...chatgptApiBrowserHeaders(auth, 'cross-site'),
      accept: '*/*',
      'content-length': String(reference.size),
      'content-type': reference.mime_type,
      'x-ms-blob-type': 'BlockBlob',
      'x-ms-version': '2020-04-08',
    },
    body: reference.base64,
    body_format: 'base64',
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
        use_case: 'multimodal',
        index_for_retrieval: false,
        file_name: reference.name,
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
  const indexed = events.find((event) => event?.event === 'file.indexing.completed')
  return {
    library_file_id: indexed?.extra?.metadata_object_id || '',
    name: indexed?.extra?.library_file_name || reference.name,
    mime_type: indexed?.extra?.mime_type || reference.mime_type,
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
  return 'png'
}

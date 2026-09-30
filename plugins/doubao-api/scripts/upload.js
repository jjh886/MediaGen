function doubaoBase64(text) {
  return host.call('crypto.base64_encode_utf8', { text }).value
}
function doubaoHex(base64) {
  return host
    .call('crypto.base64_decode_latin1', { text: base64 })
    .value.split('')
    .map((char) => char.charCodeAt(0).toString(16).padStart(2, '0'))
    .join('')
}
function doubaoHash(text) {
  return doubaoHex(
    host.call('crypto.digest_bytes', { algorithm: 'sha256', data_base64: doubaoBase64(text) }).value
  )
}
function doubaoHmac(key, text) {
  return host.call('crypto.hmac_sha256', { key_base64: key, data_base64: doubaoBase64(text) }).value
}

function doubaoImagex(session, credentials, params, body, service = 'imagex') {
  const method = body === undefined ? 'GET' : 'POST'
  const query = Object.keys(params)
    .sort()
    .map((key) => encodeURIComponent(key) + '=' + encodeURIComponent(params[key]))
    .join('&')
  const date = new Date().toISOString().replace(/[:-]|\.\d{3}/g, '')
  const day = date.slice(0, 8)
  const scope = day + '/cn-north-1/' + service + '/aws4_request'
  const payload = body === undefined ? '' : JSON.stringify(body)
  const payloadHash = doubaoHash(payload)
  const signed = { 'x-amz-date': date, 'x-amz-security-token': credentials.session_token }
  if (body !== undefined) signed['x-amz-content-sha256'] = payloadHash
  const keys = Object.keys(signed).sort()
  const canonical = [
    method,
    '/top/v1',
    query,
    keys.map((key) => key + ':' + signed[key] + '\n').join(''),
    keys.join(';'),
    payloadHash,
  ].join('\n')
  let key = doubaoBase64('AWS4' + credentials.secret_key)
  for (const part of [day, 'cn-north-1', service, 'aws4_request']) key = doubaoHmac(key, part)
  const signature = doubaoHex(
    doubaoHmac(key, 'AWS4-HMAC-SHA256\n' + date + '\n' + scope + '\n' + doubaoHash(canonical))
  )
  const headers = apiSessionHeaders(session, DOUBAO_ORIGIN, '/top/v1')
  headers.accept = '*/*'
  headers.referer = DOUBAO_ORIGIN + '/chat'
  if (body === undefined) {
    delete headers['content-type']
    delete headers.origin
  }
  const result = apiSessionRequireOk(
    host.call('http.request', {
      method,
      url: DOUBAO_ORIGIN + '/top/v1?' + query,
      headers: {
        ...headers,
        ...signed,
        authorization:
          'AWS4-HMAC-SHA256 Credential=' +
          credentials.access_key +
          '/' +
          scope +
          ', SignedHeaders=' +
          keys.join(';') +
          ', Signature=' +
          signature,
      },
      ...(body === undefined ? {} : { body: payload, body_format: 'text' }),
      proxy: 'system',
      timeout_ms: 120000,
      retry: { max_attempts: 1 },
    }),
    '豆包素材上传失败'
  )
  if (result.body?.ResponseMetadata?.Error || !result.body?.Result)
    throw new Error('upload_failed: 豆包上传签名或素材被拒绝')
  return result.body.Result
}

function doubaoUploadBinary(session, file) {
  const prepared = doubaoRequest(session, '/alice/resource/prepare_upload', {
    tenant_id: '5',
    scene_id: '5',
    resource_type: file.mime_type.startsWith('image/') ? 2 : 1,
  })
  const extension = file.name.split('.').pop().toLowerCase()
  const applied = doubaoImagex(session, prepared.upload_auth_token, {
    Action: 'ApplyImageUpload',
    Version: '2018-08-01',
    ServiceId: prepared.service_id,
    NeedFallback: 'true',
    FileSize: String(file.size),
    FileExtension: '.' + extension,
    s: Math.random().toString(36).slice(2),
  })
  const address = applied.UploadAddress
  const store = address?.StoreInfos?.[0]
  const hostname = address?.UploadHosts?.[0]
  if (!store?.StoreUri || !hostname || !/^[a-z0-9.-]+$/.test(hostname))
    throw new Error('upload_failed: 上传地址无效')
  const crc = doubaoHex(
    host.call('crypto.digest_bytes', { algorithm: 'crc32', data_base64: file.base64 }).value
  )
  const headers = apiSessionHeaders(session, DOUBAO_ORIGIN)
  delete headers.cookie
  Object.assign(headers, {
    accept: '*/*',
    'sec-fetch-site': 'cross-site',
    authorization: store.Auth,
    'content-crc32': crc,
    'content-type': 'application/octet-stream',
    // 当前网页上传 SDK 没有传 filename；素材实际名称在 pre_handle 中登记。
    'content-disposition': 'attachment; filename="undefined"',
    'x-storage-u': String(session.auth.user.user_id),
  })
  const request = { method: 'POST', url: 'https://' + hostname + '/upload/v1/' + store.StoreUri }
  doubaoCorsPreflight(session, request, [
    'authorization',
    'content-crc32',
    'content-disposition',
    'content-type',
    'x-storage-u',
  ])
  apiSessionRequireOk(
    host.call('http.request', {
      ...request,
      headers,
      body: file.base64,
      body_format: 'base64',
      proxy: 'system',
      timeout_ms: 180000,
      retry: { max_attempts: 1 },
    }),
    '上传附件失败'
  )
  const committed = doubaoImagex(
    session,
    prepared.upload_auth_token,
    {
      Action: 'CommitImageUpload',
      Version: '2018-08-01',
      ServiceId: prepared.service_id,
    },
    { SessionKey: address.SessionKey }
  )
  if (committed.Results?.[0]?.UriStatus !== 2000) throw new Error('upload_failed: 附件未完成入库')
  return committed.Results[0].Uri
}

function doubaoPrepareAttachment(session, file, messageId) {
  const uri = doubaoUploadBinary(session, file)
  const identifier = apiTextUuid()
  const isImage = file.mime_type.startsWith('image/')
  if (file.mime_type.startsWith('audio/')) {
    const registered = doubaoRequest(session, '/alice/resource/register', {
      tenant_id: '5',
      scene_id: '109',
      resource_list: [
        {
          uri,
          resource_type: 3,
          meta: {
            audio_meta: {
              file_name: file.name,
              identifier,
              size: file.size,
              parse_extra: { bot_id: '7338286299411103781', local_message_id: messageId },
            },
          },
        },
      ],
      extra: JSON.stringify({
        bot_id: '7338286299411103781',
        conversation_id: '',
        local_message_id: messageId,
      }),
    })
    if (!registered[uri]?.resource_id) throw new Error('upload_failed: 豆包音频附件登记失败')
    return {
      uri,
      identifier,
      message_id: messageId,
      name: file.name,
      size: file.size,
      is_image: false,
      is_audio: true,
      duration: file.duration,
      selected_at: file.selected_at,
      audio_attachment_id: file.audio_attachment_id,
    }
  }
  const content = isImage
    ? { image: { key: uri } }
    : {
        file: {
          file_name: file.name,
          key: uri,
          md5: doubaoHex(
            host.call('crypto.digest_bytes', { algorithm: 'md5', data_base64: file.base64 }).value
          ),
          file_type: 1,
          image: { key: uri },
          size: file.size,
        },
      }
  doubaoRequest(session, '/alice/message/pre_handle_v2_without_conv', {
    uplink_entity: { entity_type: isImage ? 2 : 1, entity_content: content, identifier },
    bot_id: '7338286299411103781',
    local_message_id: messageId,
  })
  if (!isImage) {
    const deadline = Date.now() + 600000
    let complete = false
    while (Date.now() < deadline) {
      const info = doubaoRequest(session, '/samantha/aireader/document_info/get', { uris: [uri] })
      const status = info.infos?.[uri]?.parse_info?.status
      if (status === 1) {
        complete = true
        break
      }
      if (status === 2) throw new Error('attachment_rejected: 豆包无法解析此附件')
      host.sleep(1000)
    }
    if (!complete) throw new Error('attachment_timeout: 豆包附件解析超时')
  }
  return {
    uri,
    identifier,
    message_id: messageId,
    name: file.name,
    size: file.size,
    width: file.width,
    height: file.height,
    format: file.mime_type.split('/')[1],
    is_image: isImage,
  }
}

function doubaoUploadReferences(ctx, session, options) {
  if (ctx.ref_audio_locals?.length) {
    throw new Error('invalid_input: 所选豆包网页视频模式不支持音频参考')
  }
  const videos = ctx.ref_video_locals || []
  if (videos.length > (options?.referenceMedia?.max_ref_video_count || 0))
    throw new Error('invalid_input: 所选豆包模型不支持此数量的参考视频')
  const paths = ctx.ref_image_locals || []
  const dataUrls = ctx.ref_image_dataurls || []
  const count = Math.max(paths.length, dataUrls.length)
  if (count > 10) throw new Error('invalid_input: 豆包最多支持 10 张参考图')
  const messageId = doubaoMessageId()
  const references = []
  for (let index = 0; index < count; index++) {
    let file
    if (paths[index]) {
      const data = host.call('file.read_base64', { path: paths[index] })
      const info = host.call('file.inspect_image', { path: paths[index] })
      file = {
        ...data,
        ...info,
        mime_type: info.mime,
        name: String(paths[index]).split(/[\\/]/).pop(),
      }
    } else {
      const attachment = apiTextResponsesMedia([
        { type: 'input_image', image_url: dataUrls[index] },
      ])[0]
      file = apiTextMaterializeAttachment(ctx, attachment, index)
      Object.assign(file, host.call('file.inspect_image', { path: file.path }))
    }
    if (
      !['image/png', 'image/jpeg', 'image/webp'].includes(file.mime_type) ||
      file.size > 10 * 1024 * 1024
    ) {
      throw new Error('invalid_input: 豆包参考图须为不超过 10 MiB 的 PNG、JPEG 或 WebP')
    }
    references.push(doubaoPrepareAttachment(session, file, messageId))
  }
  for (const path of videos)
    references.push(doubaoUploadVideoReference(ctx, session, path, messageId, options))
  return references
}

function doubaoUploadTextAttachments(ctx, session, request) {
  const files = request.input.flatMap((item) => item.attachments || [])
  if (files.some((attachment) => attachment.mime_type.startsWith('video/')))
    throw new Error('invalid_input: 当前豆包网页对话不支持视频文件附件')
  const messageId = doubaoMessageId()
  const prepared = files.map((attachment, index) => {
    const file = apiTextMaterializeAttachment(ctx, attachment, index)
    if (file.mime_type.startsWith('image/'))
      Object.assign(file, host.call('file.inspect_image', { path: file.path }))
    if (file.mime_type.startsWith('audio/')) {
      const info = host.call('file.inspect_media', { path: file.path })
      if (!info.has_audio || info.has_video || !(info.duration > 0))
        throw new Error('invalid_input: 音频附件内容或时长无效')
      Object.assign(file, {
        duration: info.duration,
        selected_at: Date.now(),
        audio_attachment_id: apiTextUuid(),
      })
    }
    return file
  })
  return prepared.map((file) => doubaoPrepareAttachment(session, file, messageId))
}

function doubaoReferenceBlocks(references) {
  if (!references.length) return []
  return [
    {
      block_type: 10052,
      block_id: apiTextUuid(),
      parent_id: '',
      meta_info: [],
      append_fields: [],
      content: {
        pc_event_block: '',
        attachment_block: {
          attachments: references.map((file) => ({
            type: file.is_video ? 2 : file.is_image ? 1 : 3,
            identifier: file.identifier,
            ...(file.is_video
              ? {
                  media: {
                    vid: file.vid,
                    media_type: 2,
                    duration: file.duration,
                    size: file.size,
                    name: file.name,
                    md5: file.md5,
                    resource_uri: file.uri,
                    cover: {
                      key: file.identifier,
                      image_preview: {
                        url: '',
                        width: file.width,
                        height: file.height,
                        format: '',
                        url_formats: {},
                      },
                    },
                    path: file.name,
                  },
                  image: {
                    image_thumb: { url: '', width: 0, height: 0, format: '', url_formats: {} },
                    name: '',
                    image_ori: { url: '', width: 0, height: 0, format: '', url_formats: {} },
                    uri: '',
                  },
                }
              : file.is_image
                ? {
                    image: {
                      name: file.name,
                      uri: file.uri,
                      image_ori: {
                        url: '',
                        width: file.width,
                        height: file.height,
                        format: file.format,
                        url_formats: {},
                      },
                    },
                  }
                : {
                    file: {
                      uri: file.uri,
                      url: '',
                      file_type: file.is_audio ? 1 : 0,
                      name: file.name,
                      size: file.size,
                      ...(file.is_audio ? { duration: file.duration } : {}),
                    },
                  }),
            parse_state: file.is_image ? 0 : 1,
            review_state: 1,
            upload_status: 1,
            progress: 100,
            src: '',
            ...(file.is_audio
              ? {
                  extra: {
                    extra: {
                      duration_seconds: String(file.duration),
                      audio_attachment_id: file.audio_attachment_id,
                      audio_upload_from: 'local',
                      audio_upload_way: 'file_picker',
                      audio_upload_mode: 'full_upload',
                      audio_file_extension: file.name.split('.').pop().toLowerCase(),
                      audio_file_size_bytes: String(file.size),
                      audio_duration_ms: String(file.duration * 1000),
                      audio_attachment_count: String(
                        references.filter((item) => item.is_audio).length
                      ),
                      audio_selected_at_ms: String(file.selected_at),
                      audio_resource_ready_at_ms: '0',
                    },
                  },
                }
              : {}),
          })),
        },
      },
    },
  ]
}

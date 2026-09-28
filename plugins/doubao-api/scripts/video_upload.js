function doubaoVideoPart(session, store, url, body, contentType, crc) {
  const headers = apiSessionHeaders(session, DOUBAO_ORIGIN)
  delete headers.cookie
  Object.assign(headers, {
    accept: '*/*',
    'sec-fetch-site': 'cross-site',
    authorization: store.Auth,
    'content-type': contentType,
    'x-storage-u': String(session.auth.user.user_id),
  })
  const names = ['authorization', 'content-type', 'x-storage-u']
  if (crc) {
    headers['content-crc32'] = crc
    headers['content-disposition'] = 'attachment; filename="undefined"'
    names.push('content-crc32', 'content-disposition')
  }
  const request = { method: 'POST', url }
  doubaoCorsPreflight(session, request, names)
  const response = apiSessionRequireOk(
    host.call('http.request', {
      ...request,
      headers,
      body,
      body_format: crc ? 'base64' : 'text',
      proxy: 'system',
      timeout_ms: 180000,
      retry: { max_attempts: 1 },
    }),
    '豆包参考视频上传失败'
  )
  if (Number(response.body?.code) !== 2000) throw new Error('upload_failed: 豆包参考视频分片未完成')
  return response.body.data
}

function doubaoUploadVideoReference(ctx, session, path, messageId, options) {
  const limit = options?.referenceMedia
  if (!limit?.max_ref_video_count) throw new Error('invalid_input: 所选豆包模型不支持参考视频')
  const info = host.call('file.inspect_media', { path })
  if (!info.has_video || !(info.duration > 0) || info.duration > limit.max_ref_video_duration)
    throw new Error('invalid_input: 参考视频时长超出所选模型限制')
  const file = host.call('file.read_base64', { path })
  if (!/\.(mp4|mov)$/i.test(path)) throw new Error('invalid_input: 参考视频须为 MP4 或 MOV')
  const md5 = doubaoHex(
    host.call('crypto.digest_bytes', { algorithm: 'md5', data_base64: file.base64 }).value
  )
  const prepared = doubaoRequest(session, '/alice/resource/prepare_upload', {
    tenant_id: '4',
    scene_id: '133',
    resource_type: 4,
  })
  const applied = doubaoImagex(
    session,
    prepared.upload_auth_token,
    {
      Action: 'ApplyUploadInner',
      Version: '2020-11-19',
      SpaceName: prepared.service_id,
      FileType: 'video',
      IsInner: '1',
      NeedFallback: 'true',
      FileSize: String(file.size),
      s: Math.random().toString(36).slice(2),
    },
    undefined,
    'vod'
  )
  const node = applied.InnerUploadAddress?.UploadNodes?.[0]
  const store = node?.StoreInfos?.[0]
  if (!store?.StoreUri || !store.Auth || !/^[a-z0-9.-]+$/.test(node?.UploadHost || ''))
    throw new Error('upload_failed: 豆包视频上传地址无效')
  const url = 'https://' + node.UploadHost + '/upload/v1/' + store.StoreUri
  const boundary = '----WebKitFormBoundary' + apiTextUuid().replace(/-/g, '').slice(0, 16)
  const initialized = doubaoVideoPart(
    session,
    store,
    url + '?uploadmode=part&phase=init',
    '--' + boundary + '--\r\n',
    'multipart/form-data; boundary=' + boundary
  )
  if (!initialized?.uploadid) throw new Error('upload_failed: 豆包未返回视频上传编号')
  const uploadId = encodeURIComponent(initialized.uploadid)
  const checksums = []
  const chunkSize = 5 * 1024 * 1024
  for (let offset = 0, part = 1; offset < file.size; part++) {
    let end = Math.min(offset + chunkSize, file.size)
    if (file.size - end <= chunkSize) end = file.size
    const chunk = host.call('file.read_base64', {
      path,
      offset,
      length: end - offset,
    })
    const crc = doubaoHex(
      host.call('crypto.digest_bytes', { algorithm: 'crc32', data_base64: chunk.base64 }).value
    )
    const transferred = doubaoVideoPart(
      session,
      store,
      url + '?part_number=' + part + '&uploadid=' + uploadId + '&phase=transfer',
      chunk.base64,
      'application/octet-stream',
      crc
    )
    if (transferred?.crc32 !== crc) throw new Error('upload_failed: 视频分片校验不一致')
    checksums.push(part + ':' + crc)
    offset = end
  }
  doubaoVideoPart(
    session,
    store,
    url + '?uploadmode=part&phase=finish&uploadid=' + uploadId,
    checksums.join(','),
    'text/plain;charset=UTF-8'
  )
  const committed = doubaoImagex(
    session,
    prepared.upload_auth_token,
    {
      Action: 'CommitUploadInner',
      Version: '2020-11-19',
      SpaceName: prepared.service_id,
    },
    { SessionKey: node.SessionKey, Functions: [] },
    'vod'
  )
  const video = committed.Results?.[0]
  if (!video?.Vid) throw new Error('upload_failed: 豆包参考视频未完成入库')
  return {
    identifier: apiTextUuid(),
    message_id: messageId,
    name: String(path).split(/[\\/]/).pop(),
    is_video: true,
    size: file.size,
    duration: info.duration,
    md5,
    vid: video.Vid,
    uri: 'vod://' + prepared.service_id + '/' + video.Vid,
    width: video.VideoMeta?.Width || 0,
    height: video.VideoMeta?.Height || 0,
  }
}

const API_TEXT_FILE_TYPES = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  webp: 'image/webp',
  gif: 'image/gif',
  pdf: 'application/pdf',
  txt: 'text/plain',
  md: 'text/markdown',
  csv: 'text/csv',
  json: 'application/json',
  doc: 'application/msword',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  xls: 'application/vnd.ms-excel',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  ppt: 'application/vnd.ms-powerpoint',
  pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  mp4: 'video/mp4',
  mov: 'video/quicktime',
  webm: 'video/webm',
  mp3: 'audio/mpeg',
  wav: 'audio/wav',
  m4a: 'audio/mp4',
}
for (const extension of [
  'py',
  'js',
  'jsx',
  'ts',
  'tsx',
  'go',
  'rs',
  'java',
  'c',
  'cpp',
  'h',
  'cs',
  'php',
  'rb',
  'sh',
  'sql',
  'html',
  'css',
  'xml',
  'yaml',
  'yml',
  'toml',
  'log',
]) {
  API_TEXT_FILE_TYPES[extension] = 'text/plain'
}

function apiTextResponsesMedia(content) {
  if (!Array.isArray(content)) return []
  return content.flatMap((part, index) => {
    if (!part || typeof part !== 'object') return []
    let source = ''
    if (part.type === 'input_file') {
      if (typeof part.file_data !== 'string') throw new Error('invalid_input: 附件须传入 file_data')
      source = part.file_data
      if (!source.startsWith('data:')) {
        const extension = String(part.filename || '')
          .split('.')
          .pop()
          .toLowerCase()
        const mime = part.mime_type || API_TEXT_FILE_TYPES[extension]
        if (!mime) throw new Error('invalid_input: 附件缺少可识别的文件类型')
        source = 'data:' + mime + ';base64,' + source
      }
    } else if (part.type === 'input_image') {
      source = typeof part.image_url === 'string' ? part.image_url : part.image_url?.url
    } else if (part.type === 'input_audio') {
      const format = String(part.input_audio?.format || '').toLowerCase()
      if (!['wav', 'mp3'].includes(format)) throw new Error('invalid_input: 不支持的音频格式')
      source =
        'data:' + API_TEXT_FILE_TYPES[format] + ';base64,' + String(part.input_audio?.data || '')
    } else return []
    const match = String(source || '').match(
      /^data:((?:image|video|audio|application|text)\/[a-z0-9.+-]+);base64,([A-Za-z0-9+/]+={0,2})$/i
    )
    if (!match)
      throw new Error('invalid_input: 附件须为有效 Base64 data URL；远程文件须先由宿主下载')
    const mime = match[1].toLowerCase()
    const base64 = match[2]
    const size =
      (base64.length / 4) * 3 - (base64.endsWith('==') ? 2 : base64.endsWith('=') ? 1 : 0)
    if (size <= 0 || size > 50 * 1024 * 1024 || base64.length % 4 !== 0) {
      throw new Error('invalid_input: 附件为空、编码无效或超过 50 MiB')
    }
    if (part.type === 'input_image' && !mime.startsWith('image/'))
      throw new Error('invalid_input: input_image 必须是图片')
    const extension =
      Object.keys(API_TEXT_FILE_TYPES).find((key) => API_TEXT_FILE_TYPES[key] === mime) || 'bin'
    const name = String(part.filename || 'attachment-' + (index + 1) + '.' + extension)
    if (!/^[\p{L}\p{N} _()+.-]{1,160}$/u.test(name) || name === '.' || name === '..') {
      throw new Error('invalid_input: 附件文件名无效')
    }
    return [{ name, mime_type: mime, base64, size }]
  })
}

function apiTextMaterializeAttachment(ctx, attachment, index) {
  if (typeof ctx.workspace_dir !== 'string' || !ctx.workspace_dir)
    throw new Error('invalid_input: 附件工作目录缺失')
  const extension =
    attachment.name
      .split('.')
      .pop()
      .replace(/[^a-z0-9]/gi, '') || 'bin'
  const path = ctx.workspace_dir + '/text-attachment-' + index + '.' + extension
  const saved = host.call('file.write_base64', { path, base64: attachment.base64 })
  if (saved?.size !== attachment.size) throw new Error('upload_failed: 附件落盘不完整')
  return { ...attachment, path }
}

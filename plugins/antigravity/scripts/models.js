// 协调模型有独立的上游 ID；Carbon 的目录 ID 没有 Carbon 后缀。
const AG_ORCHESTRATORS = {
  'orchestrator-neon': 'gemini-pro-orchestrator-neon',
  'orchestrator-carbon': 'gemini-pro-orchestrator',
  'orchestrator-argon': 'gemini-pro-orchestrator-argon',
}
const AG_MODELS = [
  'gemini-4-neon',
  'gemini-4-carbon',
  'gemini-4-argon',
  'gemini-4-hydrogen',
  'claude-sonnet-5-5',
  'claude-opus-5-5',
  ...Object.keys(AG_ORCHESTRATORS),
]

function agVariants(slug, catalog) {
  function available(id) {
    const model = catalog[id]
    if (!model || model.disabled === true || model.isDisabled === true) return false
    const quota = model.quotaInfo
    if (!quota) return true
    const remaining = quota.remainingFraction ?? quota.remaining
    const reset = Date.parse(quota.resetTime)
    if (remaining == null) return !(reset > Date.now())
    return remaining !== 0 || reset <= Date.now()
  }
  const coordinator = AG_ORCHESTRATORS[slug]
  if (coordinator) return available(coordinator) ? { fixed: coordinator } : {}
  const levels = ['low', 'medium', 'high', 'xhigh']
  const variants = {}
  for (const level of levels) {
    const id = slug + '-' + level
    if (available(id)) variants[level] = id
  }
  return variants
}

function agSelect(slug, catalog, effort) {
  if (!AG_MODELS.includes(slug)) throw new Error('invalid_request: 不支持该 Antigravity 模型')
  const variants = agVariants(slug, catalog)
  if (variants.fixed && effort) throw new Error('invalid_request: Orchestrator 使用固定推理等级')
  const preferred = slug.startsWith('claude-') ? 'high' : 'medium'
  const level = effort || (variants[preferred] ? preferred : Object.keys(variants)[0])
  if (!Object.hasOwn(variants, level))
    throw new Error('account_model_unavailable: 账号未开放所选模型或推理等级')
  return { model: variants[level], level: variants.fixed ? null : level }
}

// 仅公布既能通过 MediaGen 输入、又被账号上游目录接受的格式。
const AG_FILE_TYPES = {
  'image/jpeg': ['jpg', 'jpeg'],
  'image/png': ['png'],
  'image/webp': ['webp'],
  'image/gif': ['gif'],
  'image/heic': ['heic'],
  'image/heif': ['heif'],
  'application/pdf': ['pdf'],
  'text/plain': ['txt'],
  'text/csv': ['csv'],
  'text/markdown': ['md'],
  'text/html': ['html'],
  'application/json': ['json'],
  'audio/mpeg': ['mp3'],
  'audio/wav': ['wav'],
  'video/mp4': ['mp4'],
  'video/webm': ['webm'],
}
function agMimeTypes(info) {
  if (info?.supportedMimeTypes && typeof info.supportedMimeTypes === 'object')
    return Object.keys(AG_FILE_TYPES).filter((mime) => info.supportedMimeTypes[mime] === true)
  return info?.supportsImages ? ['image/jpeg', 'image/png'] : []
}

function agInputCapabilities(catalog, variants) {
  const mime = [...new Set(Object.values(variants).flatMap((id) => agMimeTypes(catalog[id])))]
  const kinds = mime.map((type) =>
    /^(image|audio|video)\//.test(type) ? type.split('/')[0] : 'file'
  )
  return {
    input_modalities: [
      ...new Set(['text', ...kinds, ...(kinds.includes('video') ? ['file'] : [])]),
    ],
    supported_mime_types: mime,
    file_extensions: mime.flatMap((type) => AG_FILE_TYPES[type]),
  }
}

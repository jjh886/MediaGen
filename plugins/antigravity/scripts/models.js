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
const AG_EXTENSIONS = {
  ...API_TEXT_FILE_TYPES,
  heic: 'image/heic',
  heif: 'image/heif',
  html: 'text/html',
  htm: 'text/html',
}
const AG_GENERATION_TIMEOUT_MS = 600000

function agMimeTypes(info) {
  if (info?.supportedMimeTypes && typeof info.supportedMimeTypes === 'object')
    return Object.keys(info.supportedMimeTypes).filter(
      (mime) =>
        info.supportedMimeTypes[mime] === true && Object.values(AG_EXTENSIONS).includes(mime)
    )
  return info?.supportsImages ? ['image/jpeg', 'image/png'] : []
}

function agInputCapabilities(catalog, variants) {
  const entries = Object.values(variants).map((id) => catalog[id])
  const mime = [...new Set(entries.flatMap(agMimeTypes))]
  const kinds = mime.map((type) =>
    /^(image|audio|video)\//.test(type) ? type.split('/')[0] : 'file'
  )
  // 基础型号只公布所有可用档位都接受的预算；带后缀的型号使用本档位预算。
  const budget = (key, fallback) =>
    Math.min(
      ...entries.map((entry) =>
        Number.isSafeInteger(entry[key]) && entry[key] > 0 ? entry[key] : fallback
      )
    )
  return {
    input_modalities: [
      ...new Set(['text', ...kinds, ...(kinds.includes('video') ? ['file'] : [])]),
    ],
    supported_mime_types: mime,
    file_extensions: Object.keys(AG_EXTENSIONS).filter((ext) => mime.includes(AG_EXTENSIONS[ext])),
    ...(kinds.includes('image') ? { image_sources: ['data_url', 'file_id'] } : {}),
    attachment_transport: 'inline',
    context_window: budget('maxTokens', 1024000),
    max_output_tokens: budget('maxOutputTokens', 65535),
    generation_timeout_ms: AG_GENERATION_TIMEOUT_MS,
  }
}

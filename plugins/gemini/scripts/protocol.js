function geminiFrames(raw) {
  const frames = []
  for (const line of String(raw || '').split('\n')) {
    if (!line.trim().startsWith('[[')) continue
    let rows
    try {
      rows = JSON.parse(line)
    } catch (_) {
      throw new Error('invalid_response: Gemini 响应帧损坏')
    }
    for (const row of rows) {
      if (row[0] === 'er') {
        const code = Number(row[2])
        const kind =
          code === 401 || code === 16
            ? 'LOGIN_EXPIRED'
            : code === 429 || code === 8
              ? 'rate_limit_exceeded'
              : code === 400
                ? 'invalid_input'
                : 'temporary_unavailable'
        throw new Error(kind + ': Gemini RPC 错误 code=' + code)
      }
      if (row[0] !== 'wrb.fr' || typeof row[2] !== 'string') continue
      try {
        frames.push({ rpc: row[1], data: JSON.parse(row[2]) })
      } catch (_) {
        throw new Error('invalid_response: Gemini 内层响应不是有效 JSON')
      }
    }
  }
  return frames
}

function geminiCatalog(session) {
  const data = geminiRpc(session, 'otAQ7b', [1])
  session.user_status = data
  const status = Number(data?.[14] || 1000)
  if (status !== 1000) {
    const category =
      status === 1016
        ? 'LOGIN_EXPIRED'
        : [1040, 1033, 1054, 1057].includes(status)
          ? 'verification_required'
          : 'account_model_unavailable'
    throw new Error(category + ': Gemini GetUserStatus 拒绝账号；code=' + status)
  }
  if (!Array.isArray(data?.[15]))
    throw new Error('upstream_client_outdated: Gemini 模型目录格式已变化')
  // 网页请求头的账号等级来自 GetUserStatus，不能把所有账号写死成免费用户。
  const products = data[16] || []
  const features = data[17] || []
  const tier =
    products.includes(16) || features.includes(106)
      ? 3
      : features.includes(198)
        ? 6
        : products.includes(8) || features.includes(19)
          ? 2
          : null
  const business = products.includes(21) ? 1 : products.includes(22) ? 2 : null
  session.account_header =
    tier === 2 || tier === 3 ? [12, tier] : business ? [13, business] : tier ? [12, tier] : null
  const rows = data[15].filter(
    (row) => /^[0-9a-f]{16}$/.test(row?.[0]) && Number.isInteger(row[17])
  )
  // 基础表描述模型家族；账号选择策略才决定实际可用名称和变体（Argon/Carbon 共用档位）。
  const choices = Array.isArray(data[24]?.[1])
    ? data[24][1]
    : rows.map((row) => [[null, row[17]], []])
  return choices.flatMap((choice) => {
    const mode = choice?.[0]?.[1]
    const row = rows.find((row) => row[17] === mode)
    if (!row) return []
    const automatic = /^(自动|auto(?:matic)?)$/i.test(choice[0][4] || row[1] || '')
    const name = automatic ? choice[0][4] || row[1] : choice[0][5] || row[12] || row[2] || row[11]
    if (typeof name !== 'string' || !name.trim()) return []
    const slug = name
      .toLowerCase()
      .replace(/[^a-z0-9.]+/g, '-')
      .replace(/^-+|-+$/g, '')
    if (!automatic && !slug) return []
    const variant = typeof choice[0][3] === 'string' ? choice[0][3] : ''
    return [
      {
        id: automatic ? 'gemini-auto' : 'gemini-' + slug,
        name,
        hex: row[0],
        mode,
        variant,
        selector: variant || row[0],
        automatic,
        efforts: (Array.isArray(choice[1]) ? choice[1] : [])
          .map((entry) => ({ 1: 'low', 4: 'medium', 2: 'high' })[entry?.[0]])
          .filter(Boolean),
      },
    ]
  })
}

function geminiModel(session, id, effort) {
  const models = geminiCatalog(session)
  const media = ['gemini-image', 'gemini-video'].includes(id)
  const model = media
    ? models.find((model) => model.mode === 1)
    : models.find((model) => model.id === id)
  if (!model) throw new Error('account_model_unavailable: 当前 Google 账号未提供所选 Gemini 模型')
  if (effort && !model.efforts.includes(effort))
    throw new Error('invalid_input: 所选 Gemini 模型不支持此思考等级')
  return {
    ...model,
    think: { low: 1, medium: 4, high: 2 }[effort] || 1,
    tool: id === 'gemini-image' ? 14 : id === 'gemini-video' ? 11 : 0,
  }
}

function geminiPayload(prompt, model, files, previous, ratio) {
  const inner = Array(99).fill(null)
  inner[0] = [prompt, 0, null, files.length ? files : null, null, null, 0]
  inner[1] = ['zh-CN']
  inner[2] = previous
    ? [
        previous?.conversation_id || '',
        previous?.response_id || '',
        previous?.candidate_id || '',
        null,
        null,
        null,
        null,
        null,
        null,
        previous?.token || '',
      ]
    : null
  for (const [key, value] of Object.entries({
    6: [0],
    7: 1,
    10: 1,
    11: 0,
    17: [[previous?.turn || 0]],
    18: 0,
    27: 1,
    30: [4, 16, 19],
    41: [1],
    53: 0,
    59: apiTextUuid().toUpperCase(),
    61: [],
    67: 0,
    68: 1,
    79: model.mode,
    80: model.think,
    91: 1,
    96: 0,
    98: 1,
  }))
    inner[Number(key)] = value
  inner[49] = model.tool
  if (model.tool === 14) {
    inner[54] = []
    inner[55] = []
  }
  if (model.tool === 11) {
    inner[0][9] = [null, null, null, null, null, null, [[null, null, null, 1]]]
    inner[54] = []
    inner[55] = [[ratio === '9:16' ? 17 : 16]]
  }
  return inner
}

function geminiParseGeneration(raw, model) {
  const result = {
    text: '',
    reasoning: '',
    conversation_id: '',
    response_id: '',
    candidate_id: '',
    token: '',
    frames: [],
  }
  for (const { data } of geminiFrames(raw)) {
    if (!Array.isArray(data)) continue
    result.frames.push(data)
    if (data[31] === true || data[31] === 1)
      throw new Error('temporary_unavailable: Gemini 返回生成失败标记 frame[31]')
    if (data[1]?.[0]) result.conversation_id = data[1][0]
    if (data[1]?.[1]) result.response_id = data[1][1]
    if (typeof data[26] === 'string') result.token = data[26]
    for (const candidate of Array.isArray(data[4]) ? data[4] : []) {
      if (candidate?.[0]) result.candidate_id = candidate[0]
      if (typeof candidate?.[1]?.[0] === 'string') result.text = candidate[1][0]
      const reasoning = candidate?.[37]?.[0]?.[0]
      if (typeof reasoning === 'string' && reasoning.length > result.reasoning.length)
        result.reasoning = reasoning
    }
    const reported = data[42]
    if (
      !model.tool &&
      !model.automatic &&
      typeof reported === 'string' &&
      reported &&
      reported !== model.name &&
      !reported.startsWith(model.name + ' ')
    )
      throw new Error(
        'account_model_unavailable: Gemini 返回模型与请求不符；requested=' +
          model.name +
          '; reported=' +
          reported
      )
  }
  return result
}

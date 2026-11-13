const GEMINI_CHANNEL = 'https://geminiweb-pa.clients6.google.com/v1/processSession'

// WebChannel 按长度标记分帧；长度使用 UTF-16 单元，不能按 UTF-8 字节切割。
function geminiChannelFrames(raw) {
  const frames = []
  let at = 0
  while (at < raw.length) {
    while (/\s/.test(raw[at] || '') && at < raw.length) at++
    if (at >= raw.length) break
    const header = /^(\d+)\n/.exec(raw.slice(at))
    if (!header) throw new Error('invalid_response: Gemini WebChannel 长度帧无效')
    at += header[0].length
    const start = at
    let depth = 0,
      quoted = false,
      escaped = false
    for (; at < raw.length; at++) {
      const c = raw[at]
      if (quoted) {
        if (escaped) escaped = false
        else if (c === '\\') escaped = true
        else if (c === '"') quoted = false
      } else if (c === '"') quoted = true
      else if (c === '[' || c === '{') depth++
      else if (c === ']' || c === '}') {
        depth--
        if (!depth) {
          at++
          break
        }
      }
    }
    if (depth || quoted) throw new Error('invalid_response: Gemini WebChannel 响应帧不完整')
    try {
      frames.push(...JSON.parse(raw.slice(start, at)))
    } catch (_) {
      throw new Error('invalid_response: Gemini WebChannel 响应帧无法解析')
    }
  }
  return frames
}

function geminiChannelURL(params) {
  return (
    GEMINI_CHANNEL +
    '?' +
    Object.entries(params)
      .map(([key, value]) => encodeURIComponent(key) + '=' + encodeURIComponent(value))
      .join('&')
  )
}

function geminiChannelPost(session, channel, offset, data) {
  return geminiRequest(
    session,
    geminiChannelURL({
      VER: 8,
      SID: channel.sid,
      gsessionid: channel.gsid,
      RID: channel.rid++,
      AID: channel.aid,
      zx: apiTextUuid(),
      t: 1,
    }),
    {
      method: 'POST',
      body_format: 'form',
      body: { count: 1, ofs: offset, req0___data__: geminiWireBase64(data) },
    }
  )
}

function geminiChannelGenerate(session, model, inner, onFrame) {
  if (!session.tokens.api_key)
    throw new Error('upstream_client_outdated: Gemini 页面未提供 WebChannel 参数')
  const metadata = Array(model.variant ? 25 : 18).fill(null)
  metadata[0] = 1
  metadata[6] = 'zh-CN'
  metadata[8] = [4, 33, 22, 5, 6, 8, 16, 19, 20]
  metadata[14] = model.mode
  metadata[15] = model.think
  metadata[16] = session.client_instance_id ||= apiTextUuid().toUpperCase()
  metadata[17] = [session.tokens.bl]
  if (model.variant) metadata[24] = model.variant
  const rpcHeaders = () => ({
    'x-goog-ext-525001261-jspb': JSON.stringify(metadata),
    'x-goog-ext-73010989-jspb': '[0]',
  })
  geminiRpc(session, 'ESY5D', [null, [5]], rpcHeaders())
  session.mode_quota = geminiRpc(session, 'VxUbXb', [], rpcHeaders())
  metadata[4] = model.selector || model.hex
  const quota = geminiRpc(session, 'aPya6c', [], rpcHeaders())
  if (quota?.[0] === true) {
    const reset = Number(quota[2]?.[0])
    const wait = Math.ceil(reset - Date.now() / 1000)
    throw new Error(
      'quota_exhausted: Gemini CheckQuota 返回额度耗尽；model=' +
        model.id +
        (wait > 0 ? '; retry after ' + wait + ' seconds' : '；上游未提供恢复时间')
    )
  }
  const binary = geminiWireBase64(
    geminiWireEncode([
      [1, 1],
      [5, model.selector || model.hex],
      [7, 'zh-CN'],
      [8, 0],
      [9, [4, 33, 22, 5, 6, 8, 16, 19, 20]],
      ...(session.account_header ? [session.account_header] : []),
      [15, model.mode],
      [16, model.think],
      [17, (session.client_instance_id ||= apiTextUuid())],
      [18, geminiWireEncode([[1, session.tokens.bl]])],
      ...(model.variant ? [[25, model.variant]] : []),
    ])
  )
  const auth = geminiHeaders(session, GEMINI_ORIGIN)
  const requestHeaders =
    'X-Goog-Api-Key:' +
    session.tokens.api_key +
    '\r\nx-goog-ext-525001261-bin:' +
    binary +
    (session.tokens.server_token ? '\r\nX-Server-Token:' + session.tokens.server_token : '') +
    '\r\nX-WebChannel-Content-Type:application/x-protobuf\r\nAuthorization:' +
    auth.authorization +
    '\r\nx-goog-ext-73010989-bin:CAA=\r\nx-goog-ext-73010990-bin:CAAQABgA\r\nx-goog-ext-525005358-bin:' +
    host.call('crypto.base64_encode_latin1', { text: '\x0a\x24' + inner[59] + '\x10\x01' }).value +
    '\r\nX-Goog-AuthUser:' +
    (session.auth.authuser || 0) +
    '\r\n'
  const channel = { rid: Math.floor(Math.random() * 80000) + 10000, aid: 0 }
  const handshake = geminiRequest(
    session,
    geminiChannelURL({
      VER: 8,
      RID: channel.rid++,
      CVER: 22,
      'X-HTTP-Session-Id': 'gsessionid',
      $httpHeaders: requestHeaders,
      zx: apiTextUuid(),
      t: 1,
    }),
    { method: 'POST', body_format: 'form', body: { count: 0 } }
  )
  channel.sid = geminiChannelFrames(handshake.body_text).find(
    (row) => row?.[1]?.[0] === 'c'
  )?.[1]?.[1]
  channel.gsid = handshake.headers?.['x-http-session-id']
  if (!channel.sid || !channel.gsid)
    throw new Error('invalid_response: Gemini WebChannel 未建立会话')
  const prompt = inner[0][0]
  inner[0][0] = ''
  const meta = geminiWireEncode([
    [1, 1],
    [2, model.selector || model.hex],
    [3, 'zh-CN'],
    [4, inner[59]],
  ])
  const setup = geminiWireEncode([
    [
      1,
      geminiWireEncode([
        [1, meta],
        [2, 0],
        [
          8,
          geminiWireEncode([
            [
              5,
              geminiWireEncode([
                [1, geminiWireInner(inner)],
                [4, 1],
              ]),
            ],
          ]),
        ],
      ]),
    ],
  ])
  const message = geminiWireEncode([
    [
      2,
      geminiWireEncode([
        [1, 1],
        [9, geminiWireEncode([[1, prompt]])],
      ]),
    ],
  ])
  host.checkpoint({ ...host.checkpoint(), phase: 'submitting' })
  geminiChannelPost(session, channel, 0, setup)
  let sent = false,
    closing = false,
    ended = false
  const receive = (rows) => {
    for (const [id, value] of rows) {
      channel.aid = Math.max(channel.aid, id)
      const status = value?.__sm__?.status?.[0]?.[0]
      if (status) {
        const code = Number(status.error?.code ?? status[0])
        if ([16, 401, 8, 429, 400].includes(code) && !(host.checkpoint() || {}).conversation_id)
          host.checkpoint({ ...host.checkpoint(), phase: 'ready' })
        throw new Error(
          (code === 16 || code === 401
            ? 'LOGIN_EXPIRED'
            : code === 8 || code === 429
              ? 'rate_limit_exceeded'
              : code === 400
                ? 'invalid_input'
                : 'temporary_unavailable') +
            ': Gemini WebChannel code=' +
            code +
            '; ' +
            String(status.error?.message || status[1] || '')
        )
      }
      const decoded = geminiWireEvent(value)
      if (decoded?.ack && !sent) {
        // 上游 ACK 早于生成状态就绪；参考实现已验证立即提交会只返回会话编号。
        host.call('time.sleep', { milliseconds: 500 })
        geminiChannelPost(session, channel, 1, message)
        sent = true
      }
      const candidate = value?.[0]?.[1]?.[6]?.[0]
      const frame = decoded?.frame || candidate?.[3]?.[3]
      if (Array.isArray(frame)) onFrame(frame)
      if ((decoded?.done || candidate?.[0] === 1) && !closing) {
        geminiChannelPost(session, channel, 2, geminiWireEncode([[4, geminiWireEncode([[1, 1]])]]))
        closing = true
      }
      if (decoded?.end || value?.[0] === 'close') ended = true
    }
    if (ended) {
      geminiRequest(
        session,
        geminiChannelURL({
          VER: 8,
          gsessionid: channel.gsid,
          SID: channel.sid,
          RID: channel.rid++,
          TYPE: 'terminate',
          zx: apiTextUuid(),
        }),
        { method: 'POST', body_format: 'text', body: '' }
      )
    }
  }
  const deadline = Date.now() + 20 * 60 * 1000
  while (!ended && Date.now() < deadline) {
    const url = geminiChannelURL({
      gsessionid: channel.gsid,
      VER: 8,
      RID: 'rpc',
      SID: channel.sid,
      AID: channel.aid,
      CI: 0,
      TYPE: 'xmlhttp',
      zx: apiTextUuid(),
      t: 1,
    })
    const response = host.stream(
      'http.sse_request',
      {
        url,
        method: 'GET',
        proxy: 'system',
        headers: geminiHeaders(session, url),
        timeout_ms: 1200000,
        collect_body: true,
        include_http_metadata: true,
        retry: { max_attempts: 1 },
      },
      (event) => {
        if (event.type === 'http_response') geminiRequireOk(event)
        else if (Array.isArray(event)) receive(event)
        return null
      }
    )
    geminiRequireOk(response)
  }
  // 媒体任务可在受理后直接结束通道，产物继续通过指定轮次的历史接口轮询。
  if (!sent || !ended || (!closing && !model.tool))
    throw new Error('temporary_unavailable: Gemini 生成尚未确认完成，请恢复原任务')
}

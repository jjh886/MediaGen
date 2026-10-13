const GROK_CLIENT_ID = 'b1a00492-073a-47ea-816f-4c329264a828'
const GROK_VERSION = '1.0.13'
const GROK_BUILD = 'https://cli-chat-proxy.grok.com/v1'
const GROK_API = 'https://api.x.ai/v1'
function grokRead(ctx) {
  return host.call('file.read_json', { path: ctx.profile_dir + '/auth.json' })
}
function grokSave(ctx, auth) {
  host.call('file.write_json', { path: ctx.profile_dir + '/auth.json', value: auth })
}
function grokError(result) {
  const code = String(result.body?.error?.code || result.body?.error || '')
  if (result.status === 426)
    throw new Error('upstream_client_outdated: Grok 要求更新插件使用的 CLI 协议版本')
  if (result.status === 401 || ['invalid_grant', 'access_denied'].includes(code))
    throw new Error('LOGIN_EXPIRED: Grok 账号需要重新授权')
  if (result.status === 429) {
    const detail = String(result.body?.error?.message || result.body?.error || '')
    if (/\(actual\/limit\):\s*\d+\/0(?:\D|$)/i.test(detail))
      throw new Error('insufficient_quota: 当前模型的上游账号额度为 0，请检查 API 额度或订阅')
    throw new Error('rate_limit_exceeded: HTTP 429 Grok 请求受限')
  }
  if (result.status === 400) {
    if (/file content.*unsupported.*ZDR/i.test(String(result.body?.error || '')))
      throw new Error('account_permission_denied: 当前账号的数据保留策略不允许文件输入')
    throw new Error('invalid_input: Grok 拒绝了请求参数')
  }
  if (result.status === 403)
    throw new Error('account_permission_denied: HTTP 403 Grok 账号无此接口权限')
  throw new Error('HTTP ' + result.status + ': Grok 上游请求失败')
}
function grokAuth(ctx) {
  host.call('lock.acquire', { key: ctx.profile_dir + ':grok-auth' })
  try {
    const auth = grokRead(ctx)
    if (!auth.access_token) throw new Error('LOGIN_EXPIRED: 请先登录 Grok')
    const valid = Number(auth.expires_at || 0) > Date.now() + 60000
    // 手动刷新同时更新授权，避免升级会员后继续沿用旧令牌中的权益。
    if (valid && (!ctx.refresh_quota || !auth.refresh_token)) return auth
    if (!auth.refresh_token) throw new Error('LOGIN_EXPIRED: Grok 授权已过期')
    const result = host.call('http.request', {
      method: 'POST',
      url: 'https://auth.x.ai/oauth2/token',
      fingerprint: 'none',
      body_format: 'form',
      body: {
        grant_type: 'refresh_token',
        client_id: auth.client_id || GROK_CLIENT_ID,
        refresh_token: auth.refresh_token,
      },
      timeout_ms: 30000,
    })
    if (!result.ok) grokError(result)
    if (!result.body?.access_token) throw new Error('LOGIN_EXPIRED: Grok 刷新响应缺少令牌')
    const fresh = {
      ...auth,
      access_token: result.body.access_token,
      refresh_token: result.body.refresh_token || auth.refresh_token,
      expires_at: Date.now() + Number(result.body.expires_in || 3600) * 1000,
    }
    grokSave(ctx, fresh)
    return fresh
  } finally {
    host.call('lock.release', { key: ctx.profile_dir + ':grok-auth' })
  }
}
function grokHeaders(ctx, auth, build = false) {
  const headers = {
    authorization: 'Bearer ' + auth.access_token,
    'content-type': 'application/json',
    accept: 'application/json',
    'user-agent': build
      ? 'xai-grok-workspace/' + GROK_VERSION
      : 'xai-grok-build/' + GROK_VERSION + ' (linux; x86_64)',
  }
  if (build) {
    const requestId = String(ctx.request_id || ctx.task_id || '').replace(/^req_/, '')
    const wireId = /^[0-9a-f]{32}$/i.test(requestId)
      ? grokUUID(requestId)
      : host.call('crypto.random_uuid', {}).value
    Object.assign(headers, {
      'x-xai-token-auth': 'xai-grok-cli',
      'x-grok-client-version': GROK_VERSION,
      'x-grok-client-identifier': 'grok-shell',
      'x-grok-client-mode': 'interactive',
      'x-grok-agent-id': auth.agent_id,
      'x-grok-req-id': wireId,
      'x-client-request-id': ctx.request_id || ctx.task_id || wireId,
      'x-authenticateresponse': 'authenticate-response',
    })
    if (auth.user_id) headers['x-grok-user-id'] = auth.user_id
    if (ctx.request?.model) headers['x-grok-model-override'] = ctx.request.model
    const cache = String(
      ctx.request?.options?.session_key || ctx.request?.prompt_cache_key || ''
    ).trim()
    if (cache) {
      const hash = host.call('crypto.sha256_hex', { text: 'mediagen:grok:session:' + cache }).value
      const session = /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(cache)
        ? cache.toLowerCase()
        : grokUUID(hash.slice(0, 12) + '8' + hash.slice(13, 16) + 'a' + hash.slice(17, 32))
      headers['x-grok-session-id'] = headers['x-grok-conv-id'] = session
    }
  }
  return headers
}
function grokUUID(hex) {
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20, 32),
  ].join('-')
}
function grokRequest(ctx, auth, path, body, options = {}) {
  const build = options.build === true
  const result = host.call('http.request', {
    method: body === undefined ? 'GET' : 'POST',
    url: (build ? GROK_BUILD : GROK_API) + path,
    fingerprint: 'none',
    headers: grokHeaders(ctx, auth, build),
    body,
    timeout_ms: 180000,
    ...options,
  })
  if (!result.ok) {
    if (
      body === undefined &&
      /^\/videos\/[^/]+$/.test(path) &&
      [404, 410].includes(result.status)
    ) {
      host.checkpoint({ phase: 'ready' })
      throw new Error('temporary_unavailable: 原 Grok 视频任务无法查询，将重新提交请求')
    }
    if (
      result.status >= 400 &&
      result.status < 500 &&
      result.status !== 408 &&
      host.checkpoint()?.phase === 'submitting'
    )
      host.checkpoint({ phase: 'ready' })
    grokError(result)
  }
  return result
}
function grokAsset(url) {
  if (!/^https:\/\/(?:[\w.-]+\.)?(?:x\.ai|grok\.com)\//i.test(url))
    throw new Error('Grok 媒体地址无效')
  const response = host.call('http.request', {
    method: 'GET',
    url,
    fingerprint: 'none',
    response_body_format: 'base64',
    timeout_ms: 120000,
  })
  if (!response.ok || !response.body_base64) throw new Error('Grok 媒体下载失败')
  return response
}

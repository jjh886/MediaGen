const GROK_CLIENT_ID = 'b1a00492-073a-47ea-816f-4c329264a828'
const GROK_VERSION = '1.0.4'
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
  if (result.status === 401 || ['invalid_grant', 'access_denied'].includes(code))
    throw new Error('LOGIN_EXPIRED: Grok 账号需要重新授权')
  if (result.status === 429) throw new Error('rate_limit_exceeded: HTTP 429 Grok 请求受限')
  if (result.status === 400) throw new Error('invalid_input: Grok 拒绝了请求参数')
  if (result.status === 403)
    throw new Error('account_permission_denied: HTTP 403 Grok 账号无此接口权限')
  throw new Error('HTTP ' + result.status + ': Grok 上游请求失败')
}
function grokAuth(ctx) {
  host.call('lock.acquire', { key: ctx.profile_dir + ':grok-auth' })
  try {
    const auth = grokRead(ctx)
    if (!auth.access_token) throw new Error('LOGIN_EXPIRED: 请先登录 Grok')
    if (Number(auth.expires_at || 0) > Date.now() + 60000) return auth
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
    'user-agent': (build ? 'grok-shell/' : 'xai-grok-build/') + GROK_VERSION + ' (linux; x86_64)',
  }
  if (build) {
    Object.assign(headers, {
      'x-xai-token-auth': 'xai-grok-cli',
      'x-grok-client-version': GROK_VERSION,
      'x-grok-client-identifier': 'grok-shell',
      'x-grok-client-mode': 'headless',
      'x-grok-agent-id': auth.agent_id,
      'x-grok-req-id': ctx.request_id || ctx.task_id || '',
      'x-authenticateresponse': 'authenticate-response',
    })
    if (auth.user_id) headers['x-grok-user-id'] = auth.user_id
    const cache = ctx.request?.prompt_cache_key
    if (typeof cache === 'string' && /^[0-9a-f-]{36}$/i.test(cache))
      headers['x-grok-session-id'] = headers['x-grok-conv-id'] = cache
  }
  return headers
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
  if (!result.ok) grokError(result)
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

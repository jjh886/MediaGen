function main(ctx) {
  const path = ctx.profile_dir + '/login.json'
  const pending = host.call('file.read_json', { path })
  if (Date.now() > pending.expires_at) throw new Error('Grok 授权码已过期，请重新登录')
  if (Date.now() < pending.next_poll_at) return { loggedIn: false }
  if (!pending.device_code) {
    const auth = host.call('file.read_json', { path: ctx.profile_dir + '/auth.json' })
    if (!auth.access_token || !auth.user_id) throw new Error('Grok 授权流程尚未初始化，请重新登录')
    return {
      loggedIn: true,
      userId: auth.user_id,
      email: auth.email || '',
      username: auth.email || auth.user_id,
      tier: 'free',
    }
  }
  const result = host.call('http.request', {
    method: 'POST',
    url: 'https://auth.x.ai/oauth2/token',
    fingerprint: 'none',
    headers: pending.headers,
    body_format: 'form',
    body: {
      grant_type: 'urn:ietf:params:oauth:grant-type:device_code',
      client_id: pending.client_id,
      device_code: pending.device_code,
    },
    timeout_ms: 30000,
  })
  const token = result.body || {}
  if (token.error === 'authorization_pending' || token.error === 'slow_down') {
    if (token.error === 'slow_down') pending.interval = Number(pending.interval || 5) + 5
    pending.next_poll_at = Date.now() + Number(pending.interval || 5) * 1000
    host.call('file.write_json', { path, value: pending })
    return { loggedIn: false }
  }
  if (!result.ok || !token.access_token) throw new Error('Grok 授权失败，请确认授权或重新登录')
  let claims = {}
  const encoded = String(token.id_token || token.access_token).split('.')[1]
  if (encoded) {
    try {
      claims = JSON.parse(host.call('crypto.base64url_decode_utf8', { text: encoded }).value)
    } catch (_) {
      /* 身份展示不影响服务端令牌的使用。 */
    }
  }
  const userId = String(claims.sub || claims.user_id || '')
  if (!userId) throw new Error('Grok 授权缺少账号身份')
  const agentId = host.call('crypto.random_uuid', {}).value
  host.call('file.write_json', {
    path: ctx.profile_dir + '/auth.json',
    value: {
      access_token: token.access_token,
      refresh_token: token.refresh_token,
      expires_at: Date.now() + Number(token.expires_in || 3600) * 1000,
      client_id: pending.client_id,
      user_id: userId,
      email: claims.email || '',
      agent_id: agentId,
      scope: token.scope || '',
    },
  })
  host.call('file.write_json', { path, value: {} })
  return {
    loggedIn: true,
    userId,
    email: claims.email || '',
    username: claims.email || userId,
    tier: 'free',
  }
}

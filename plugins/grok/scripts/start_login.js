function main(ctx) {
  const headers = {
    'x-grok-client-version': GROK_VERSION,
    'x-grok-client-surface': 'ui',
    accept: 'application/json',
  }
  const result = host.call('http.request', {
    method: 'POST',
    url: 'https://auth.x.ai/oauth2/device/code',
    headers,
    fingerprint: 'none',
    body_format: 'form',
    body: {
      client_id: GROK_CLIENT_ID,
      scope:
        'openid profile email offline_access grok-cli:access api:access conversations:read conversations:write workspaces:read workspaces:write',
      referrer: 'grok-build',
    },
    timeout_ms: 30000,
  })
  if (!result.ok) grokError(result)
  const value = result.body
  if (!value?.device_code || !value.user_code || !value.verification_uri)
    throw new Error('Grok 授权响应无效')
  host.call('file.write_json', {
    path: ctx.profile_dir + '/login.json',
    value: {
      ...value,
      client_id: GROK_CLIENT_ID,
      headers,
      expires_at: Date.now() + Number(value.expires_in || 1800) * 1000,
      next_poll_at: 0,
    },
  })
  return {
    start_url: value.verification_uri_complete || value.verification_uri,
    instructions: '请完成 Grok 授权；如果页面要求验证码，请输入 ' + value.user_code,
  }
}

function main(ctx) {
  if (!ctx.oauth || !ctx.oauth.code)
    throw new Error('Codex 登录需要先在远程登录窗口完成 OAuth 授权。')
  const oauth = ctx.oauth
  const response = host.call('http.request', {
    method: 'POST',
    url: oauth.token_url,
    proxy: 'system',
    headers: codexAuthHeaders(ctx),
    body_format: 'form',
    body: {
      grant_type: 'authorization_code',
      code: oauth.code,
      redirect_uri: oauth.redirect_uri,
      client_id: oauth.client_id,
      code_verifier: oauth.code_verifier,
    },
    timeout_ms: 30000,
    retry: { max_attempts: 1 },
  })
  if (!response.ok || !response.body?.access_token)
    throw new Error('LOGIN_EXPIRED: Codex 授权换取令牌失败，请重新登录。')
  const auth = codexBuildAuthFile(response.body, {})
  host.call('file.write_json', { path: ctx.profile_dir + '/auth.json', value: auth })
  const account = { ...codexAccountPatch(auth, {}), account_id: codexAccountId(auth) }
  return { loggedIn: true, tier: 'free', account: { ...account, loggedIn: true } }
}

function main(ctx) {
  if (!ctx.oauth || !ctx.oauth.code) {
    throw new Error('Codex 登录需要先在远程登录窗口完成 OAuth 授权。')
  }
  const token = exchangeToken(ctx.oauth)
  return saveAccount(ctx.profile_dir, token)
}

function exchangeToken(oauth) {
  return host.call('http.request', {
    method: 'POST',
    url: oauth.token_url,
    proxy: 'system',
    body_format: 'form',
    body: {
      grant_type: 'authorization_code',
      code: oauth.code,
      redirect_uri: oauth.redirect_uri,
      client_id: oauth.client_id,
      code_verifier: oauth.code_verifier,
    },
    timeout_ms: 30000,
  }).body
}

function saveAccount(profileDir, token) {
  const account = buildAccount(token)
  host.call('file.write_json', {
    path: profileDir + '/auth.json',
    value: buildAuthFile(token, account.account_id),
  })
  return {
    loggedIn: true,
    userId: account.user_id,
    username: account.username,
    email: account.email,
    tier: 'free',
    account,
  }
}

function buildAccount(token) {
  const idPayload = decodeJwtPayload(token.id_token)
  const accessPayload = decodeJwtPayload(token.access_token)
  const accountId = firstNonEmpty([
    get(accessPayload, ['https://api.openai.com/auth', 'account_id']),
    get(accessPayload, ['https://api.openai.com/auth', 'chatgpt_account_id']),
    get(idPayload, ['https://api.openai.com/auth', 'account_id']),
    get(idPayload, ['https://api.openai.com/auth', 'chatgpt_account_id']),
  ])
  const email = firstNonEmpty([idPayload.email, accessPayload.email])
  const userId = firstNonEmpty([idPayload.sub, accessPayload.sub, accountId])
  const username = firstNonEmpty([email, accountId, 'codex'])
  return {
    api_token: token.access_token,
    refresh_token: token.refresh_token,
    email,
    user_id: userId,
    account_id: accountId,
    username,
    name: username,
  }
}

function buildAuthFile(token, accountId) {
  return {
    OPENAI_API_KEY: null,
    tokens: {
      id_token: token.id_token,
      access_token: token.access_token,
      refresh_token: token.refresh_token,
      account_id: accountId,
    },
    last_refresh: new Date().toISOString(),
  }
}

function decodeJwtPayload(token) {
  if (!token) return {}
  const parts = String(token).split('.')
  if (parts.length < 2) return {}
  const payload = host.call('crypto.base64url_decode_utf8', { text: parts[1] }).value
  return JSON.parse(payload)
}

function firstNonEmpty(values) {
  for (const value of values) {
    if (typeof value === 'string' && value.trim()) return value
  }
  return ''
}

function get(value, path) {
  let current = value
  for (const key of path) {
    if (!current || typeof current !== 'object') return ''
    current = current[key]
  }
  return typeof current === 'string' ? current : ''
}

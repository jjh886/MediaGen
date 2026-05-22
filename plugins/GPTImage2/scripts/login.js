const CLIENT_ID = 'app_EMoamEEZ73f0CkXaXp7hrann'
const AUTH_URL = 'https://auth.openai.com/oauth/authorize'
const TOKEN_URL = 'https://auth.openai.com/oauth/token'

function main(ctx) {
  let server = null
  try {
    const flow = prepareLogin(ctx)
    server = flow.server
    const callback = waitForOAuthCallback(server.server_id, flow.state)
    const token = exchangeToken(callback.code, server.redirect_uri, flow.codeVerifier)
    return saveAccount(ctx.profile_dir, token)
  } finally {
    safeCall('browser.close', { profile_dir: ctx.profile_dir, force: true })
    if (server && server.server_id) {
      safeCall('callback_server.stop', { server_id: server.server_id })
    }
  }
}

function prepareLogin(ctx) {
  const codeVerifier = randomToken(64)
  const state = randomToken(32)
  const codeChallenge = host.call('crypto.sha256_base64url', { text: codeVerifier }).value
  const server = startCallbackServer()
  host.call('browser.launch_external', {
    url: buildLoginUrl(server.redirect_uri, codeChallenge, state),
    profile_dir: ctx.profile_dir,
    chrome_path: ctx.chrome_path,
    force_restart: true,
  })
  return { codeVerifier, state, server }
}

function startCallbackServer() {
  const params = {
    port: 1455,
    path: '/auth/callback',
    markers: ['frame64-callback-server', 'frame64-gptimage2-callback'],
    redirect_host: 'localhost',
  }
  const first = host.tryCall('callback_server.start', params)
  if (first.success) return first.data
  host.call('process.kill_port', { port: 1455 })
  return host.call('callback_server.start', params)
}

function buildLoginUrl(redirectUri, codeChallenge, state) {
  return (
    AUTH_URL +
    '?' +
    queryString({
      response_type: 'code',
      client_id: CLIENT_ID,
      redirect_uri: redirectUri,
      scope: 'openid profile email offline_access',
      code_challenge: codeChallenge,
      code_challenge_method: 'S256',
      id_token_add_organizations: true,
      codex_cli_simplified_flow: true,
      state,
      originator: 'codex_vscode',
    })
  )
}

function waitForOAuthCallback(serverId, expectedState) {
  while (true) {
    const request = host.call('callback_server.wait_request', {
      server_id: serverId,
      timeout_ms: 300000,
    })
    if (request.query.state !== expectedState) {
      host.call('callback_server.respond', {
        server_id: serverId,
        request_id: request.request_id,
        status: 400,
        body: htmlPage('登录校验失败，请回到应用重新开始。'),
      })
      continue
    }
    if (request.query.error) {
      const message = request.query.error_description || request.query.error
      host.call('callback_server.respond', {
        server_id: serverId,
        request_id: request.request_id,
        status: 400,
        body: htmlPage('登录失败：' + escapeHtml(message)),
      })
      throw new Error(message)
    }
    if (request.query.code) {
      host.call('callback_server.respond', {
        server_id: serverId,
        request_id: request.request_id,
        status: 200,
        body: htmlPage('登录已完成，可以关闭这个窗口。'),
      })
      return { code: request.query.code }
    }
    host.call('callback_server.respond', {
      server_id: serverId,
      request_id: request.request_id,
      status: 400,
      body: htmlPage('登录回调缺少授权码，请重新登录。'),
    })
  }
}

function exchangeToken(code, redirectUri, codeVerifier) {
  return host.call('http.request', {
    method: 'POST',
    url: TOKEN_URL,
    proxy: 'system',
    body_format: 'form',
    body: {
      grant_type: 'authorization_code',
      code,
      redirect_uri: redirectUri,
      client_id: CLIENT_ID,
      code_verifier: codeVerifier,
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
  return account
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
  const username = firstNonEmpty([email, accountId, 'GPTImage2'])
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

function randomToken(bytes) {
  return host.call('crypto.random_base64url', { bytes }).value
}

function decodeJwtPayload(token) {
  if (!token) return {}
  const parts = String(token).split('.')
  if (parts.length < 2) return {}
  const payload = host.call('crypto.base64url_decode_utf8', { text: parts[1] }).value
  return JSON.parse(payload)
}

function queryString(values) {
  const parts = []
  for (const key of Object.keys(values)) {
    if (values[key] === undefined || values[key] === null || values[key] === '') continue
    parts.push(encodeURIComponent(key) + '=' + encodeURIComponent(String(values[key])))
  }
  return parts.join('&')
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

function safeCall(action, params) {
  try {
    host.tryCall(action, params)
  } catch (_) {}
}

function htmlPage(message) {
  return (
    '<!doctype html><meta charset="utf-8"><title>GPTImage2 登录</title><p>' +
    message +
    '</p><script>setTimeout(()=>window.close(),800)</script>'
  )
}

function escapeHtml(text) {
  return String(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

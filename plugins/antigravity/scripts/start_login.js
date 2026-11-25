function main(ctx) {
  const client = agRegistration(ctx)
  const state = host.call('crypto.random_base64url', { bytes: 32 }).value
  const verifier = host.call('crypto.random_base64url', { bytes: 64 }).value
  const reader = host.call('crypto.random_base64url', { bytes: 32 }).value
  const challenge = host.call('crypto.sha256_base64url', { text: verifier }).value
  const params = {
    response_type: 'code',
    client_id: client.client_id,
    redirect_uri: AG_REDIRECT,
    state,
    scope:
      'https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile',
    code_challenge: challenge,
    code_challenge_method: 'S256',
    access_type: 'offline',
    prompt: 'consent select_account',
  }
  const key = 'antigravity-login:' + ctx.profile_dir
  host.call('lock.acquire', { key })
  try {
    agListen(ctx.profile_dir, state, reader)
    try {
      host.call('file.write_json', {
        path: ctx.profile_dir + '/login.json',
        value: {
          state,
          reader,
          verifier,
          client_id: client.client_id,
          expires_at: Date.now() + 600000,
        },
      })
    } catch (error) {
      agCallback(reader, true)
      throw error
    }
  } finally {
    host.call('lock.release', { key })
  }
  return {
    start_url:
      'https://accounts.google.com/o/oauth2/v2/auth?' +
      Object.entries(params)
        .map(([k, v]) => k + '=' + encodeURIComponent(v))
        .join('&'),
  }
}

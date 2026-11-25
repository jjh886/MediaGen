function main(ctx) {
  const key = 'antigravity-login:' + ctx.profile_dir
  host.call('lock.acquire', { key })
  try {
    return agFinishLogin(ctx)
  } finally {
    host.call('lock.release', { key })
  }
}

function agFinishLogin(ctx) {
  const path = ctx.profile_dir + '/login.json'
  const record = host.tryCall('file.read_json', { path })
  const pending = record.data
  if (!record.success || !pending?.state) throw new Error('请从 MediaGen 打开 Antigravity 登录窗口')
  const stored = host.tryCall('file.read_json', { path: ctx.profile_dir + '/auth.json' })
  if (stored.success && stored.data?.login_state === pending.state && stored.data.account)
    return { loggedIn: true, account: stored.data.account }
  if (pending.expires_at <= Date.now()) throw new Error('Antigravity 登录会话已过期，请重新登录')
  let auth = pending.auth
  if (!auth) {
    const callback = agCallback(pending.reader)
    if (!callback.success || !callback.data?.ok) throw new Error('本地回调监听已不可用，请重新登录')
    const result = callback.data.body
    if (result?.denied) {
      agCallback(pending.reader, true)
      throw new Error('Google 授权已取消')
    }
    if (!result?.code) throw new Error('正在等待 Antigravity 授权回调')
    try {
      auth = agExchange(ctx, {
        grant_type: 'authorization_code',
        code: result.code,
        code_verifier: pending.verifier,
        redirect_uri: AG_REDIRECT,
        client_id: pending.client_id,
      })
    } catch (error) {
      agCallback(pending.reader, true)
      host.call('file.write_json', { path, value: {} })
      throw error
    }
    host.call('file.write_json', {
      path,
      value: {
        reader: pending.reader,
        state: pending.state,
        expires_at: pending.expires_at,
        auth,
      },
    })
    agCallback(pending.reader, true)
  }
  const info = agRequireOk(
    host.call('http.request', agParams(auth, AG_USERINFO)),
    '读取 Google 账号'
  )
  auth.account = agAccountProfile(info)
  auth.project = agLoadProject(auth)
  auth.login_state = pending.state
  host.call('file.write_json', { path: ctx.profile_dir + '/auth.json', value: auth })
  host.call('file.write_json', { path, value: { state: pending.state } })
  return { loggedIn: true, account: auth.account }
}

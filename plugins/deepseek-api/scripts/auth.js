const DEEPSEEK_ORIGIN = 'https://chat.deepseek.com'

function deepseekHeaders(session, path = '/', method = 'POST') {
  if (!session.auth.access_token) throw new Error('LOGIN_EXPIRED: DeepSeek 缺少登录令牌')
  const headers = {
    ...apiSessionHeaders(session, DEEPSEEK_ORIGIN, path),
    accept: '*/*',
    referer: session.conversationId
      ? DEEPSEEK_ORIGIN + '/a/chat/s/' + encodeURIComponent(session.conversationId)
      : DEEPSEEK_ORIGIN + '/',
    authorization: 'Bearer ' + session.auth.access_token,
    'x-client-bundle-id': 'com.deepseek.chat',
    'x-client-platform': 'web',
    'x-client-locale': 'zh_CN',
    'x-client-version': '2.5.0',
    'x-client-timezone-offset': String(session.auth.fingerprint.timezone_offset_seconds || 0),
    'x-device-id': session.auth.device_id || '',
    'x-device-model': '',
    ...(session.auth.client_headers || {}),
  }
  if (method === 'GET') {
    delete headers['content-type']
    delete headers.origin
  }
  if (path.startsWith('/api/v0/client/settings') && session.auth.settings_token)
    headers['x-settings-token'] = session.auth.settings_token
  for (const key of ['leim', 'dliq']) {
    const value = session.auth.hif?.[key]?.value
    if (typeof value === 'string' && value && value.length <= 16384) headers['x-hif-' + key] = value
  }
  return headers
}

function deepseekRequest(session, path, body, method, timeoutMs = 60000) {
  const result = apiSessionRequireOk(
    host.call('http.request', {
      method: method || 'POST',
      url: DEEPSEEK_ORIGIN + '/api/v0' + path,
      headers: deepseekHeaders(session, '/api/v0' + path.split('?')[0], method || 'POST'),
      proxy: 'system',
      timeout_ms: timeoutMs,
      retry: { max_attempts: 1 },
      ...(body === undefined ? {} : { body }),
    }),
    'DeepSeek 请求失败'
  )
  deepseekRefreshSession(session, result, '/api/v0' + path.split('?')[0])
  const value = result.body
  const globalCode = Number(value?.code || 0)
  const code = globalCode || Number(value?.data?.biz_code || 0)
  if ([40002, 40003].includes(globalCode)) throw new Error('LOGIN_EXPIRED: DeepSeek 登录已失效')
  if (code !== 0) {
    if (/频繁|rate|limit/i.test(value?.data?.biz_msg || value?.msg || '')) {
      throw new Error('rate_limit_exceeded: DeepSeek 请求过于频繁')
    }
    throw new Error('platform_rejected: DeepSeek 错误码 ' + code)
  }
  return value?.data?.biz_data ?? value?.data ?? value
}

function deepseekRefreshSession(session, result, path) {
  let changed = apiSessionUpdateCookies(
    session,
    result.headers,
    'chat.deepseek.com',
    path,
    'deepseek.com'
  )
  const token = result.headers?.['x-settings-token']
  if (typeof token === 'string' && token && token !== session.auth.settings_token) {
    session.auth.settings_token = token
    changed = true
  }
  if (path === '/api/v0/client/settings') {
    const delay = Number(result.headers?.['x-fetch-after-sec'])
    session.settingsRefreshAt =
      Number.isFinite(delay) && delay > 0 ? Date.now() + delay * 1000 : Infinity
  }
  if (changed) host.call('file.write_json', { path: session.auth_path, value: session })
}

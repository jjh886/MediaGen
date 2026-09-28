const DEEPSEEK_ORIGIN = 'https://chat.deepseek.com'

function deepseekHeaders(session, path = '/') {
  if (!session.auth.access_token) throw new Error('LOGIN_EXPIRED: DeepSeek 缺少登录令牌')
  return {
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
}

function deepseekRequest(session, path, body, method) {
  const result = apiSessionRequireOk(
    host.call('http.request', {
      method: method || 'POST',
      url: DEEPSEEK_ORIGIN + '/api/v0' + path,
      headers: deepseekHeaders(session, '/api/v0' + path.split('?')[0]),
      proxy: 'system',
      timeout_ms: 60000,
      retry: { max_attempts: 1 },
      ...(body === undefined ? {} : { body }),
    }),
    'DeepSeek 请求失败'
  )
  deepseekRefreshSession(session, result, '/api/v0' + path.split('?')[0])
  const value = result.body
  const code = value?.data?.biz_code ?? value?.code
  if ([40001, 40002, 40003].includes(Number(code)))
    throw new Error('LOGIN_EXPIRED: DeepSeek 登录已失效')
  if (Number(value?.code || 0) !== 0 || Number(code || 0) !== 0) {
    if (/频繁|rate|limit/i.test(value?.data?.biz_msg || value?.msg || '')) {
      throw new Error('rate_limit_exceeded: DeepSeek 请求过于频繁')
    }
    throw new Error('platform_rejected: DeepSeek 错误码 ' + code)
  }
  return value?.data?.biz_data ?? value?.data ?? value
}

function deepseekRefreshSession(session, result, path) {
  if (apiSessionUpdateCookies(session, result.headers, 'chat.deepseek.com', path, 'deepseek.com'))
    host.call('file.write_json', { path: session.auth_path, value: session })
}

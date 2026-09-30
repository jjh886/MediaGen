function doubaoSignedRequest(session, path, body, method = 'POST', extraHeaders = {}) {
  const security = session.auth.security
  if (!security?.storage?.xmst || !session.auth.fingerprint?.screen) {
    throw new Error('LOGIN_EXPIRED: 请重新登录以采集豆包完整签名身份')
  }
  if (!session.securitySdk) {
    if (
      !/^https:\/\/lf-flow-web-cdn\.doubao\.com\/obj\/flow-doubao\/(?:doubao\/chat\/static\/js\/async\/bdms-sdk\.[a-z0-9]+\.js|rc-client-security\/web\/stable\/[a-zA-Z0-9._-]+\/bdms\.js)$/.test(
        security.sdk_url
      )
    ) {
      throw new Error('invalid_response: 豆包安全 SDK 地址不受支持')
    }
    session.securitySdk = apiSessionRequireOk(
      host.call('http.request', {
        method: 'GET',
        url: security.sdk_url,
        headers: { 'user-agent': session.auth.fingerprint.user_agent },
        proxy: 'system',
        timeout_ms: 60000,
      }),
      '读取豆包安全 SDK 失败'
    ).body_text
    if (!session.securitySdk || session.securitySdk.length > 2 * 1024 * 1024)
      throw new Error('invalid_response: 豆包安全 SDK 无效')
  }
  const url =
    doubaoRequestOrigin(path) + path + (path.includes('?') ? '&' : '?') + doubaoQuery(session, path)
  const headers = doubaoHeaders(session, path.split('?')[0])
  if (path.startsWith('/im/')) headers['content-type'] = 'application/json; encoding=utf-8'
  Object.assign(headers, extraHeaders)
  const payload =
    body === undefined
      ? method === 'POST' && path.startsWith('/passport/')
        ? ''
        : undefined
      : JSON.stringify(body)
  if (!session.securityRuntime) {
    Object.defineProperty(session, 'securityRuntime', {
      value: doubaoSecurityRuntime({
        sdk: session.securitySdk,
        fingerprint: session.auth.fingerprint,
        read_cookie: () =>
          apiSessionCookies(
            { cookies: session.cookies.filter((cookie) => !cookie.httpOnly) },
            'www.doubao.com',
            '/chat/'
          ),
        write_cookie: (cookie) => doubaoWriteScriptCookie(session, cookie),
        storage: security.storage,
        session_storage: security.session_storage,
        started_at: Date.now(),
        url: DOUBAO_ORIGIN + '/chat/',
        send: (request) => doubaoSecuritySend(session, request),
        drain_jobs: () => host.drain_jobs(),
        config: {
          aid: 497858,
          pageId: 26930,
          paths: {
            include: [
              '/alice',
              '/samantha',
              '/passport',
              '/biz',
              '/chat/completion',
              '/chat/async/chunk_stream',
            ],
            exclude: [
              '/samantha/notice/info',
              '/samantha/user/preference/get',
              '/samantha/user/ab/get',
              '/samantha/plugin/recommend/webtodesktop',
              '/samantha/guidance/get_task',
              '/samantha/guidance/draw_result',
            ],
          },
          ic: 13,
          ddrt: 13,
        },
      }),
    })
  }
  // 这些请求头由浏览器网络栈补充，不应作为 fetch 的 JS 入参交给 SDK。
  const scriptHeaders = { ...headers }
  for (const name of Object.keys(scriptHeaders)) {
    if (
      ['cookie', 'user-agent', 'accept-language', 'origin', 'referer'].includes(name) ||
      name.startsWith('sec-')
    )
      delete scriptHeaders[name]
  }
  const signed = session.securityRuntime.sign({
    url,
    method,
    headers: scriptHeaders,
    body: payload,
  })
  doubaoPersistSecurityStorage(session)

  return {
    method,
    url: signed.url,
    headers: {
      ...headers,
      ...signed.headers,
      cookie: doubaoHeaders(session, path.split('?')[0]).cookie,
    },
    ...(signed.body === undefined ? {} : { body: signed.body, body_format: 'text' }),
    proxy: 'system',
    retry: { max_attempts: 1 },
  }
}

function doubaoWriteScriptCookie(session, line) {
  const name = line.split('=', 1)[0].trim()
  if (session.cookies.some((cookie) => cookie.name === name && cookie.httpOnly)) return
  const writable = line.replace(/;\s*httponly(?:\s*;|\s*$)/gi, ';')
  if (
    apiSessionUpdateCookies(
      session,
      { 'set-cookie': writable },
      'www.doubao.com',
      '/chat/',
      'doubao.com'
    )
  )
    host.call('file.write_json', { path: session.auth_path, value: session })
}

function doubaoRefreshSecurity(session, result, path = '/') {
  session.securityRuntime?.complete(result)
  doubaoPersistSecurityStorage(session)
  let changed = apiSessionUpdateCookies(
    session,
    result.headers,
    doubaoRequestOrigin(path).slice('https://'.length),
    path.split('?')[0],
    'doubao.com'
  )
  const token = result.headers?.['x-ms-token']
  if (typeof token === 'string' && token && token !== session.auth.security.storage.xmst) {
    session.auth.security.storage.xmst = token
    if (session.securityRuntime) session.securityRuntime.storage.xmst = token
    changed = true
  }
  if (changed) host.call('file.write_json', { path: session.auth_path, value: session })
  if (typeof doubaoPassportRefresh === 'function') doubaoPassportRefresh(session, result)
}

function doubaoPersistSecurityStorage(session) {
  const storage = session.securityRuntime?.storage
  const sessionStorage = session.securityRuntime?.session_storage
  if (
    !storage ||
    (JSON.stringify(storage) === JSON.stringify(session.auth.security.storage) &&
      JSON.stringify(sessionStorage) === JSON.stringify(session.auth.security.session_storage))
  )
    return
  session.auth.security.storage = { ...storage }
  session.auth.security.session_storage = { ...sessionStorage }
  host.call('file.write_json', { path: session.auth_path, value: session })
}

function doubaoSecuritySend(session, request) {
  if (!/^https:\/\/mssdk\.bytedance\.com\/web\/(?:r\/token|common)(?:\?|$)/.test(request.url))
    throw new Error(
      'unsupported_protocol: 豆包安全 SDK 使用了尚未适配的请求接口 ' +
        String(request.url).split('?')[0].slice(0, 160)
    )
  const headers = apiSessionHeaders(session, DOUBAO_ORIGIN)
  delete headers.cookie
  Object.assign(headers, request.headers, {
    accept: '*/*',
    'sec-fetch-site': 'cross-site',
  })
  if (request.with_credentials) {
    const cookie = apiSessionCookies(session, 'mssdk.bytedance.com', '/web/')
    if (cookie) headers.cookie = cookie
  }
  const result = host.call('http.request', {
    method: request.method,
    url: request.url,
    headers,
    ...(request.body === undefined ? {} : { body: request.body, body_format: 'text' }),
    proxy: 'system',
    timeout_ms: 60000,
    retry: { max_attempts: 1 },
  })
  if (
    request.with_credentials &&
    apiSessionUpdateCookies(
      session,
      result.headers,
      'mssdk.bytedance.com',
      '/web/',
      'bytedance.com'
    )
  )
    host.call('file.write_json', { path: session.auth_path, value: session })
  return result
}

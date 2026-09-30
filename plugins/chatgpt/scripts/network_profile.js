function chatgptApiTlsProfile(auth) {
  const version = Number(
    String(chatgptApiFingerprint(auth).user_agent || '').match(/Chrome\/(\d+)/)?.[1]
  )
  const supported = [
    100, 101, 104, 105, 106, 107, 108, 109, 110, 114, 116, 117, 118, 119, 120, 123, 124, 126, 127,
    128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146,
    147, 148, 149,
  ]
  return 'chrome_' + (supported.includes(version) ? version : 149)
}

function chatgptApiRequireNoChallenge(result, operation) {
  const challenge =
    Object.entries(result?.headers || {}).some(
      ([name, value]) => name.toLowerCase() === 'cf-mitigated' && value === 'challenge'
    ) ||
    /(?:window\._cf_chl_opt|\/cdn-cgi\/challenge-platform\/)/.test(String(result?.body_text || ''))
  if (challenge)
    throw new Error(
      'verification_required: ' + operation + ' 需要完成 ChatGPT 安全验证，请打开账号验证窗口后继续'
    )
}

// 将传输 UA 和 Client Hints 与实际可用的 TLS 画像绑定，保留账号设备和硬件身份。
function chatgptApiAlignFingerprint(auth) {
  const fp = auth.auth.fingerprint || (auth.auth.fingerprint = {})
  const major = chatgptApiTlsProfile(auth).slice(7)
  const version = major + '.0.0.0'
  fp.user_agent = /Chrome\/\d+/.test(fp.user_agent || '')
    ? fp.user_agent.replace(/Chrome\/[\d.]+/, 'Chrome/' + version)
    : 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/' +
      version +
      ' Safari/537.36'
  fp.ua_full_version = version
  fp.ua_brands = [
    { brand: 'Not:A-Brand', version: '99' },
    { brand: 'Chromium', version: major },
    { brand: 'Google Chrome', version: major },
  ]
  fp.ua_full_version_list = fp.ua_brands.map((brand) => ({
    ...brand,
    version: brand.brand === 'Not:A-Brand' ? '99.0.0.0' : version,
  }))
}

function chatgptApiRememberCookies(auth, response, requestPath = '/') {
  const raw = response?.headers?.['set-cookie']
  if (!raw || !Array.isArray(auth.cookies)) return response
  const key = 'chatgpt-auth:' + auth.storage_path
  const locked = auth.storage_path && !auth.refreshing
  if (locked) host.call('lock.acquire', { key })
  try {
    if (locked) auth.cookies = host.call('file.read_json', { path: auth.storage_path }).cookies
    for (const line of String(raw).split('\n')) {
      const [pair, ...attributes] = line.split(';')
      const index = pair.indexOf('=')
      if (index <= 0) continue
      const next = {
        name: pair.slice(0, index).trim(),
        value: pair.slice(index + 1).trim(),
        domain: 'chatgpt.com',
        path: requestPath.slice(0, requestPath.lastIndexOf('/')) || '/',
        expires: -1,
        secure: false,
      }
      let maxAge
      for (const attribute of attributes) {
        const [rawKey, ...rest] = attribute.trim().split('=')
        const name = rawKey.toLowerCase(),
          value = rest.join('=')
        if (name === 'domain') next.domain = value.toLowerCase().replace(/^\./, '')
        if (name === 'path' && value.startsWith('/')) next.path = value
        if (name === 'secure') next.secure = true
        if (name === 'httponly') next.httpOnly = true
        if (name === 'max-age' && /^-?\d+$/.test(value)) maxAge = Number(value)
        if (name === 'expires' && Number.isFinite(Date.parse(value)))
          next.expires = Date.parse(value) / 1000
      }
      if (next.domain !== 'chatgpt.com') continue
      if (maxAge !== undefined) next.expires = maxAge <= 0 ? 0 : Date.now() / 1000 + maxAge
      auth.cookies = auth.cookies.filter(
        (cookie) =>
          !(
            cookie.name === next.name &&
            String(cookie.domain || 'chatgpt.com').replace(/^\./, '') === next.domain &&
            (cookie.path || '/') === next.path
          )
      )
      if (next.expires < 0 || next.expires > Date.now() / 1000) auth.cookies.push(next)
    }
    if (locked) {
      const latest = host.call('file.read_json', { path: auth.storage_path })
      host.call('file.write_json', {
        path: auth.storage_path,
        value: { ...latest, cookies: auth.cookies },
      })
    }
    return response
  } finally {
    if (locked) host.call('lock.release', { key })
  }
}

function chatgptApiCookieHeader(auth) {
  const now = Date.now() / 1000
  const path = arguments[1] || '/'
  return auth.cookies
    .filter((cookie) => {
      const domain = String(cookie.domain || 'chatgpt.com').replace(/^\./, '')
      const scope = cookie.path || '/'
      return (
        cookie.name &&
        cookie.value &&
        domain === 'chatgpt.com' &&
        (path === scope ||
          (path.startsWith(scope) && (scope.endsWith('/') || path[scope.length] === '/'))) &&
        (Number(cookie.expires) < 0 ||
          !Number.isFinite(Number(cookie.expires)) ||
          Number(cookie.expires) > now)
      )
    })
    .sort((a, b) => (b.path || '/').length - (a.path || '/').length)
    .map((cookie) => cookie.name + '=' + cookie.value)
    .join('; ')
}

function chatgptApiSaveAuth(auth) {
  if (!auth.storage_path) return
  host.call('file.write_json', {
    path: auth.storage_path,
    value: {
      version: 1,
      captured_at_unix_ms: auth.captured_at_unix_ms,
      auth: auth.auth,
      cookies: auth.cookies,
    },
  })
}

// 请求、校验和写回分开处理，让各阶段的失败原因保持清晰。

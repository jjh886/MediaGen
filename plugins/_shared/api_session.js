function apiSessionRead(ctx) {
  const stored = host.call('file.read_json', { path: ctx.profile_dir + '/auth.json' })
  if (!stored?.auth || !Array.isArray(stored.cookies)) {
    throw new Error('LOGIN_EXPIRED: 登录凭据缺失，请重新登录。')
  }
  Object.defineProperty(stored, 'auth_path', { value: ctx.profile_dir + '/auth.json' })
  Object.defineProperty(stored, 'securitySdk', { value: undefined, writable: true })
  return stored
}

function apiSessionCookies(session, hostname, pathname = '/') {
  return session.cookies
    .filter((cookie) => {
      const domain = String(cookie.domain || '').replace(/^\./, '')
      return (
        cookie.name &&
        cookie.value &&
        (hostname === domain || (!cookie.hostOnly && hostname.endsWith('.' + domain))) &&
        (!cookie.path ||
          pathname === cookie.path ||
          pathname.startsWith(cookie.path.endsWith('/') ? cookie.path : cookie.path + '/')) &&
        (!cookie.expires || cookie.expires <= 0 || cookie.expires > Date.now() / 1000)
      )
    })
    .map((cookie) => cookie.name + '=' + cookie.value)
    .join('; ')
}

function apiSessionHeaders(session, origin, pathname = '/') {
  const fp = session.auth.fingerprint || {}
  const headers = {
    accept: 'application/json',
    'content-type': 'application/json',
    'user-agent': fp.user_agent || '',
    'accept-language': (fp.languages?.length ? fp.languages : ['zh-CN', 'zh'])
      .slice(0, 10)
      .map((language, index) => language + (index ? ';q=' + (1 - index / 10).toFixed(1) : ''))
      .join(','),
    origin,
    referer: origin + '/',
    'sec-fetch-site': 'same-origin',
    'sec-fetch-mode': 'cors',
    'sec-fetch-dest': 'empty',
  }
  if (fp.ua_brands?.length)
    headers['sec-ch-ua'] = fp.ua_brands
      .map((item) => '"' + item.brand + '";v="' + item.version + '"')
      .join(', ')
  if (fp.ua_platform) headers['sec-ch-ua-platform'] = '"' + fp.ua_platform + '"'
  headers['sec-ch-ua-mobile'] = fp.ua_mobile ? '?1' : '?0'
  const cookie = apiSessionCookies(session, origin.replace(/^https:\/\//, ''), pathname)
  if (cookie) headers.cookie = cookie
  return headers
}

function apiSessionUpdateCookies(
  session,
  headers,
  hostname,
  pathname = '/',
  allowedDomain = hostname
) {
  let changed = false
  for (const line of String(headers?.['set-cookie'] || '').split('\n')) {
    const [pair, ...attributes] = line.split(';')
    const equals = pair.indexOf('=')
    if (equals <= 0) continue
    const name = pair.slice(0, equals).trim()
    const value = pair.slice(equals + 1).trim()
    if (!/^[!#$%&'*+.^_`|~\w-]+$/.test(name) || /[\r\n\x00]/.test(value)) continue
    const cookie = {
      name,
      value,
      domain: hostname,
      hostOnly: true,
      path: pathname.slice(0, pathname.lastIndexOf('/')) || '/',
      secure: false,
      httpOnly: false,
    }
    let maxAge
    for (const attribute of attributes) {
      const [key, ...rest] = attribute.trim().split('=')
      const text = rest.join('=').trim()
      if (key.toLowerCase() === 'domain') {
        cookie.domain = text.toLowerCase().replace(/^\./, '')
        cookie.hostOnly = false
      }
      if (key.toLowerCase() === 'path' && text.startsWith('/')) cookie.path = text
      if (key.toLowerCase() === 'secure') cookie.secure = true
      if (key.toLowerCase() === 'httponly') cookie.httpOnly = true
      if (key.toLowerCase() === 'expires' && Number.isFinite(Date.parse(text)))
        cookie.expires = Date.parse(text) / 1000
      if (key.toLowerCase() === 'max-age' && /^-?\d+$/.test(text)) maxAge = Number(text)
    }
    if (hostname !== cookie.domain && !hostname.endsWith('.' + cookie.domain)) continue
    // 账号只接收自己平台的域，不能把登录 Cookie 扩展到公共后缀。
    if (cookie.domain !== allowedDomain && !cookie.domain.endsWith('.' + allowedDomain)) continue
    if (maxAge !== undefined) cookie.expires = Date.now() / 1000 + maxAge
    session.cookies = session.cookies.filter(
      (old) =>
        old.name !== name ||
        String(old.domain).replace(/^\./, '') !== cookie.domain ||
        (old.path || '/') !== cookie.path
    )
    if (cookie.expires === undefined || cookie.expires > Date.now() / 1000)
      session.cookies.push(cookie)
    changed = true
  }
  return changed
}

function apiSessionRequireOk(result, label) {
  if (result?.status === 401) throw new Error('LOGIN_EXPIRED: ' + label)
  if (result?.status === 429) throw new Error('rate_limit_exceeded: ' + label + ' HTTP 429')
  if (!result?.ok) throw new Error('AUTH_INCONCLUSIVE: ' + label + ' HTTP ' + result?.status)
  return result
}

function apiSessionEventData(event) {
  if (event?.data && typeof event.data === 'object') return event.data
  try {
    return JSON.parse(event?.data || '{}')
  } catch (_) {
    throw new Error('invalid_response: 平台事件不是有效 JSON')
  }
}

function apiSessionHealthError(error) {
  const expired = String(error?.message || '').startsWith('LOGIN_EXPIRED:')
  return {
    logged_in: expired ? false : null,
    status: expired ? 'login_expired' : 'inconclusive',
    reason: expired ? '登录已失效，请重新登录' : '暂时无法确认账号状态，请稍后重试',
  }
}

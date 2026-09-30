;(async () => {
  const resources = performance.getEntriesByType('resource').map((entry) => entry.name)
  const accountUrl = doubaoBrowserAccountUrl(resources)
  const response = await fetch(accountUrl.href, {
    credentials: 'include',
    headers: { 'content-type': 'application/json' },
  })
  const body = await response.json()
  const user = body.data
  const userId = user?.user_id_str || user?.user_id
  if (!response.ok || !userId) throw new Error('豆包登录状态尚未确认，请在页面完成登录后重试')
  const sdk =
    resources.find((url) =>
      /\/rc-client-security\/web\/stable\/[a-zA-Z0-9._-]+\/bdms\.js$/.test(url)
    ) || resources.find((url) => /\/bdms-sdk\.[a-z0-9]+\.js$/.test(url))
  const queryResource = resources
    .slice()
    .reverse()
    .find(
      (url) =>
        url.startsWith('https://www.doubao.com/') &&
        url.includes('web_tab_id=') &&
        url.includes('web_id=')
    )
  if (!sdk || !queryResource || !localStorage.getItem('xmst'))
    throw new Error('豆包安全会话尚未初始化，请稍后重试')
  const keys = [
    'version_code',
    'language',
    'device_platform',
    'doubao_device_platform',
    'aid',
    'real_aid',
    'pkg_type',
    'device_id',
    'pc_version',
    'doubao_pc_version',
    'web_id',
    'tea_uuid',
    'region',
    'sys_region',
    'samantha_web',
    'web_platform',
    'use-olympus-account',
    'web_tab_id',
  ]
  const query = new URL(queryResource).searchParams
  const fp = document.cookie
    .split(';')
    .map((item) => item.trim())
    .find((item) => item.startsWith('s_v_web_id='))
    ?.slice(11)
  if (!fp || !query.get('web_id')) throw new Error('豆包网页身份尚未初始化，请稍后重试')
  const passportRequests = {}
  const accountConfigSource = (window.__LOADABLE_LOADED_CHUNKS__ || [])
    .flatMap((chunk) => Object.values(chunk[1] || {}))
    .filter((module) => typeof module === 'function')
    .map((module) => module.toString())
    .find((source) => source.includes('getPassportSDKConfig') && /appKey\s*:\s*["']/.test(source))
  const passportAppKey = accountConfigSource?.match(/appKey\s*:\s*["']([a-f0-9]{32})["']/)?.[1]
  if (!passportAppKey) throw new Error('豆包账号 SDK 配置尚未初始化，请稍后重试')
  for (const path of ['/passport/token/beat/web/', '/passport/doubao/security_token_beat/']) {
    const resource = resources
      .slice()
      .reverse()
      .find((url) => url.startsWith('https://accounts.doubao.com' + path + '?'))
    if (!resource) throw new Error('豆包账号续期协议尚未初始化，请稍后重试')
    passportRequests[path] = Object.fromEntries(
      [...new URL(resource).searchParams].filter(
        ([key]) =>
          ![
            'sign',
            'qs',
            'msToken',
            'a_bogus',
            'ts',
            'p_ts',
            'p_no',
            'biz_trace_id',
            'scene',
          ].includes(key)
      )
    )
  }
  return JSON.stringify({
    user: {
      user_id: String(userId),
      name: user.screen_name || user.name || user.mobile || user.email || '',
    },
    account_query: Object.fromEntries(accountUrl.searchParams),
    passport_requests: passportRequests,
    passport_app_key: passportAppKey,
    protocol_query: {
      ...Object.fromEntries(
        keys.filter((key) => query.has(key)).map((key) => [key, query.get(key)])
      ),
      fp: decodeURIComponent(fp),
      tz_name: Intl.DateTimeFormat().resolvedOptions().timeZone,
    },
    security: {
      sdk_url: sdk,
      storage: {
        xmst: localStorage.getItem('xmst'),
        __msuuid__: localStorage.getItem('__msuuid__'),
      },
      session_storage: {
        __msuuid__: sessionStorage.getItem('__msuuid__'),
      },
    },
    identity: JSON.parse(localStorage.getItem('samantha_web_web_id') || '{}'),
    tea: JSON.parse(localStorage.getItem('__tea_cache_tokens_497858') || '{}'),
    fingerprint: await apiLoginFingerprint(),
  })
})()

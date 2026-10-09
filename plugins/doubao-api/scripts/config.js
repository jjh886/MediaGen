function doubaoBrowserAccountUrl(resources) {
  const observed = resources
    .slice()
    .reverse()
    .find((url) => url.startsWith('https://accounts.doubao.com/passport/account/info/v2/?'))
  const url = new URL(observed || 'https://accounts.doubao.com/passport/account/info/v2/')
  if (!observed) {
    // 已登录页面可直接使用服务端注入的用户信息，不一定再次请求 account/info。
    const common = resources
      .slice()
      .reverse()
      .find((entry) => entry.startsWith('https://www.doubao.com/') && entry.includes('web_id='))
    if (!common) throw new Error('豆包账号协议尚未初始化，请稍后重试')
    const query = new URL(common).searchParams
    for (const key of [
      'pc_version',
      'version_code',
      'language',
      'device_platform',
      'aid',
      'real_aid',
      'pkg_type',
      'use-olympus-account',
      'region',
      'sys_region',
    ]) {
      if (!query.has(key)) throw new Error('豆包账号参数缺失：' + key)
      url.searchParams.set(key, query.get(key))
    }
    // 当前网页账号初始化协议的固定字段；版本随平台适配更新。
    url.searchParams.set('account_sdk_source', 'web')
    url.searchParams.set('sdk_version', '2.2.10')
    url.searchParams.set('doubao_from', 'inject_html')
  }
  url.searchParams.delete('msToken')
  url.searchParams.delete('a_bogus')
  return url
}

function doubaoBrowserPassportCapture(resources) {
  const passportRequests = {}
  const accountConfigSource = (window.__LOADABLE_LOADED_CHUNKS__ || [])
    .flatMap((chunk) => Object.values(chunk[1] || {}))
    .filter((module) => typeof module === 'function')
    .map((module) => module.toString())
    .find(
      (source) =>
        (source.includes('getPassportSDKConfig') ||
          (source.includes('accounts.doubao.com') && source.includes('isOversea'))) &&
        /appKey\s*:\s*["']/.test(source)
    )
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
  return { passport_requests: passportRequests, passport_app_key: passportAppKey }
}

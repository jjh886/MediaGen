// 该脚本只在“添加账号/重新登录”完成时运行。返回值由服务端以 0600 权限写入账号目录，
// 不进入账号公开字段，也不会写入日志。
;(async () => {
  const response = await fetch('/api/auth/session', {
    credentials: 'include',
    cache: 'no-store',
    headers: { accept: 'application/json' },
  })
  if (!response.ok) throw new Error(`ChatGPT session HTTP ${response.status}`)
  const session = await response.json()
  if (!session?.accessToken || !session?.account?.id) {
    throw new Error('ChatGPT session 缺少 accessToken 或 account.id')
  }

  const scripts = Array.from(document.scripts || [])
    .map((item) => item?.src || '')
    .filter(Boolean)
  const pageBuild = document.documentElement?.getAttribute('data-build') || ''
  const pageSequence = document.documentElement?.getAttribute('data-seq') || ''
  const navigatorPrototype = Object.getPrototypeOf(navigator)
  const navigatorKeys = navigatorPrototype ? Object.keys(navigatorPrototype) : []
  // UA Client Hints 由真实登录浏览器读取，后续 HTTP 请求不能只伪造一个 User-Agent。
  const uaData = navigator.userAgentData
  const uaHighEntropy = uaData?.getHighEntropyValues
    ? await uaData.getHighEntropyValues([
        'architecture',
        'bitness',
        'fullVersionList',
        'model',
        'platformVersion',
        'uaFullVersion',
      ])
    : {}

  return JSON.stringify({
    access_token: session.accessToken,
    expires: session.expires || '',
    account: session.account || {},
    user: session.user || {},
    fingerprint: {
      user_agent: navigator.userAgent || '',
      language: navigator.language || 'en-US',
      languages: Array.from(navigator.languages || [navigator.language || 'en-US']),
      platform: navigator.platform || '',
      hardware_concurrency: navigator.hardwareConcurrency || 8,
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
      timezone_offset_min: new Date().getTimezoneOffset(),
      screen_width: screen.width || 1366,
      screen_height: screen.height || 768,
      page_width: window.innerWidth || 1366,
      page_height: window.innerHeight || 768,
      pixel_ratio: window.devicePixelRatio || 1,
      page_build: pageBuild,
      page_sequence: pageSequence,
      ua_brands: Array.from(uaData?.brands || []),
      ua_full_version_list: Array.from(uaHighEntropy.fullVersionList || []),
      ua_full_version: uaHighEntropy.uaFullVersion || '',
      ua_mobile: Boolean(uaData?.mobile),
      ua_architecture: uaHighEntropy.architecture || '',
      ua_bitness: uaHighEntropy.bitness || '',
      ua_model: uaHighEntropy.model || '',
      ua_platform: uaData?.platform || '',
      ua_platform_version: uaHighEntropy.platformVersion || '',
      script_sources: scripts,
      document_keys: Object.keys(document),
      window_keys: Object.keys(window),
      navigator_keys: navigatorKeys,
      local_storage_keys: Object.keys(localStorage || {}),
      history_length: history.length || 1,
      time_origin: performance.timeOrigin || Date.now(),
    },
  })
})()

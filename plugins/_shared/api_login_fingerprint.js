async function apiLoginFingerprint() {
  const ua = navigator.userAgentData
  const high = ua?.getHighEntropyValues
    ? await ua.getHighEntropyValues([
        'architecture',
        'bitness',
        'fullVersionList',
        'platformVersion',
        'uaFullVersion',
      ])
    : {}
  return {
    user_agent: navigator.userAgent,
    languages: Array.from(navigator.languages),
    language: navigator.language,
    platform: navigator.platform,
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    timezone_offset_seconds: -new Date().getTimezoneOffset() * 60,
    vendor: navigator.vendor,
    app_name: navigator.appName,
    app_code_name: navigator.appCodeName,
    product: navigator.product,
    product_sub: navigator.productSub,
    vendor_sub: navigator.vendorSub,
    webdriver: navigator.webdriver,
    plugins: Array.from(navigator.plugins || [], (plugin) => ({
      name: plugin.name,
      filename: plugin.filename,
      description: plugin.description,
      mime_types: Array.from(plugin, (mime) => ({
        type: mime.type,
        suffixes: mime.suffixes,
        description: mime.description,
      })),
    })),
    hardware_concurrency: navigator.hardwareConcurrency,
    device_memory: navigator.deviceMemory,
    max_touch_points: navigator.maxTouchPoints,
    screen: {
      width: screen.width,
      height: screen.height,
      availWidth: screen.availWidth,
      availHeight: screen.availHeight,
      colorDepth: screen.colorDepth,
      pixelDepth: screen.pixelDepth,
    },
    inner_width: innerWidth,
    inner_height: innerHeight,
    outer_width: outerWidth,
    outer_height: outerHeight,
    screen_x: screenX,
    screen_y: screenY,
    device_pixel_ratio: devicePixelRatio,
    referrer: document.referrer,
    ua_brands: Array.from(ua?.brands || []),
    ua_platform: ua?.platform || '',
    ua_mobile: Boolean(ua?.mobile),
    ua_full_version_list: high.fullVersionList || [],
    ua_full_version: high.uaFullVersion || '',
    ua_architecture: high.architecture || '',
    ua_bitness: high.bitness || '',
    ua_platform_version: high.platformVersion || '',
  }
}

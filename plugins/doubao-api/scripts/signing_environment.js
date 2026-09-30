// 字节 SDK 在独立计算环境中运行；设备值来自该账号登录时采集的真实指纹。
// SDK 的网络回调只通过显式传入的 HTTP 适配器执行，不依赖浏览器进程。
function doubaoSecurityRuntime(input) {
  const scope = globalThis
  const fp = input.fingerprint
  const storage = { ...input.storage }
  const sessionStorageValues = { ...input.session_storage }
  function base64Decode(text) {
    const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
    let bits = 0,
      value = 0,
      output = ''
    for (const char of String(text).replace(/=+$/, '')) {
      const digit = alphabet.indexOf(char)
      if (digit < 0) continue
      value = (value << 6) | digit
      bits += 6
      if (bits >= 8) {
        bits -= 8
        output += String.fromCharCode((value >> bits) & 255)
      }
    }
    return output
  }
  function base64Encode(text) {
    const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
    let output = ''
    for (let index = 0; index < text.length; index += 3) {
      const a = text.charCodeAt(index),
        b = text.charCodeAt(index + 1),
        c = text.charCodeAt(index + 2)
      const value = (a << 16) | ((b || 0) << 8) | (c || 0)
      output +=
        alphabet[(value >> 18) & 63] +
        alphabet[(value >> 12) & 63] +
        (Number.isNaN(b) ? '=' : alphabet[(value >> 6) & 63]) +
        (Number.isNaN(c) ? '=' : alphabet[value & 63])
    }
    return output
  }
  function address(value) {
    const absolute = /^https?:\/\//.test(value)
      ? value
      : 'https://www.doubao.com' + (value.startsWith('/') ? '' : '/') + value
    const match = absolute.match(/^(https?:)\/\/([^/]+)([^?#]*)(\?[^#]*)?(#.*)?$/)
    if (!match) throw new Error('签名地址无效')
    return {
      href: absolute,
      protocol: match[1],
      host: match[2],
      hostname: match[2],
      port: '',
      origin: match[1] + '//' + match[2],
      pathname: match[3] || '/',
      search: match[4] || '',
      hash: match[5] || '',
    }
  }
  function element(tag) {
    const node = {
      tagName: tag.toUpperCase(),
      style: {},
      children: [],
      setAttribute(key, value) {
        this[key] = String(value)
      },
      getAttribute(key) {
        return this[key] || null
      },
      appendChild(child) {
        this.children.push(child)
        return child
      },
      removeChild(child) {
        return child
      },
      getElementsByTagName() {
        return []
      },
      addEventListener() {},
      removeEventListener() {},
    }
    if (tag === 'a') {
      let value = address(input.url)
      for (const key of [
        'href',
        'protocol',
        'host',
        'hostname',
        'port',
        'origin',
        'pathname',
        'search',
        'hash',
      ]) {
        Object.defineProperty(node, key, {
          configurable: true,
          get: () => value[key],
          set: (next) => {
            value = address(String(next))
          },
        })
      }
    }
    return node
  }
  class Headers {
    constructor(values = {}) {
      this.values =
        values instanceof Headers
          ? { ...values.values }
          : Array.isArray(values)
            ? Object.fromEntries(values)
            : { ...values }
    }
    get(name) {
      const key = Object.keys(this.values).find((key) => key.toLowerCase() === name.toLowerCase())
      return key ? this.values[key] : null
    }
    set(name, value) {
      this.values[name] = String(value)
    }
    forEach(callback) {
      for (const [key, value] of Object.entries(this.values)) callback(value, key)
    }
  }
  class Request {
    constructor(url, options = {}) {
      this.url = String(url)
      Object.assign(this, options)
      this.headers = new Headers(options.headers)
    }
    clone() {
      return new Request(this.url, this)
    }
  }
  const transport = doubaoSecurityTransport(input, Headers)
  function createStorage(values) {
    const methods = {
      getItem: (key) => (Object.hasOwn(values, String(key)) ? values[String(key)] : null),
      setItem: (key, value) => {
        values[String(key)] = String(value)
      },
      removeItem: (key) => {
        delete values[String(key)]
      },
      clear: () => {
        for (const key of Object.keys(values)) delete values[key]
      },
      key: (index) => Object.keys(values)[Number(index) >>> 0] ?? null,
    }
    return new Proxy(methods, {
      get: (target, key) =>
        key === 'length'
          ? Object.keys(values).length
          : Object.hasOwn(target, key)
            ? target[key]
            : values[key],
      set: (_target, key, value) => {
        values[String(key)] = String(value)
        return true
      },
      deleteProperty: (_target, key) => {
        delete values[key]
        return true
      },
      ownKeys: () => Object.keys(values),
      getOwnPropertyDescriptor: (_target, key) =>
        Object.hasOwn(values, key)
          ? { configurable: true, enumerable: true, writable: true, value: values[key] }
          : undefined,
    })
  }
  function namedCollection(items, key) {
    Object.defineProperties(items, {
      item: { value: (index) => items[index] || null },
      namedItem: { value: (name) => items.find((item) => item[key] === name) || null },
    })
    for (const item of items) {
      if (!(item[key] in items)) Object.defineProperty(items, item[key], { value: item })
    }
    return items
  }
  const plugins = Array.isArray(fp.plugins)
    ? namedCollection(
        fp.plugins.map((plugin) => {
          const value = namedCollection(
            plugin.mime_types.map((mime) => ({ ...mime })),
            'type'
          )
          Object.assign(value, {
            name: plugin.name,
            filename: plugin.filename,
            description: plugin.description,
          })
          for (const mime of value) mime.enabledPlugin = value
          return value
        }),
        'name'
      )
    : undefined
  const document = {
    referrer: fp.referrer || '',
    readyState: 'complete',
    visibilityState: 'visible',
    hidden: false,
    createElement: element,
    addEventListener() {},
    removeEventListener() {},
    querySelector() {
      return null
    },
    querySelectorAll() {
      return []
    },
    getElementsByTagName() {
      return []
    },
    documentElement: element('html'),
    head: element('head'),
    body: element('body'),
  }
  Object.defineProperty(document, 'cookie', {
    get: () => (input.read_cookie ? input.read_cookie() : input.cookie),
    set: (value) => {
      if (!input.write_cookie) throw new Error('安全 SDK 缺少 Cookie 写入适配器')
      input.write_cookie(String(value))
    },
  })
  Object.assign(scope, {
    window: scope,
    self: scope,
    top: scope,
    parent: scope,
    document,
    location: address('https://www.doubao.com/chat/'),
    navigator: {
      userAgent: fp.user_agent,
      appVersion: fp.user_agent.replace(/^Mozilla\//, ''),
      platform: fp.platform,
      language: fp.language,
      languages: fp.languages,
      vendor: fp.vendor,
      appName: fp.app_name,
      appCodeName: fp.app_code_name,
      product: fp.product,
      productSub: fp.product_sub,
      vendorSub: fp.vendor_sub,
      plugins,
      hardwareConcurrency: fp.hardware_concurrency,
      deviceMemory: fp.device_memory,
      cookieEnabled: true,
      onLine: true,
      webdriver: fp.webdriver,
      maxTouchPoints: fp.max_touch_points || 0,
    },
    screen: fp.screen,
    localStorage: createStorage(storage),
    sessionStorage: createStorage(sessionStorageValues),
    Headers,
    Request,
    XMLHttpRequest: transport.XMLHttpRequest,
    atob: base64Decode,
    btoa: base64Encode,
    addEventListener() {},
    removeEventListener() {},
    setTimeout: transport.setTimeout,
    clearTimeout: transport.clearTimeout,
    setInterval: transport.setInterval,
    clearInterval: transport.clearInterval,
    requestAnimationFrame: transport.requestAnimationFrame,
    cancelAnimationFrame: transport.cancelAnimationFrame,
    performance: { now: () => Date.now() - input.started_at },
    fetch: transport.fetch,
    innerWidth: fp.inner_width,
    innerHeight: fp.inner_height,
    outerWidth: fp.outer_width,
    outerHeight: fp.outer_height,
    screenX: fp.screen_x,
    screenY: fp.screen_y,
    devicePixelRatio: fp.device_pixel_ratio,
  })
  document.location = scope.location
  ;(0, eval)(input.sdk)
  // 新版直接注册 bdms；旧版资源通过异步模块注册。使用实际采集的资源对应的入口。
  const modules = scope.__LOADABLE_LOADED_CHUNKS__?.[0]?.[1]
  const cache = {}
  function requireModule(id) {
    if (cache[id]) return cache[id].exports
    const module = { exports: {} }
    cache[id] = module
    if (!modules[id]) throw new Error('安全 SDK 依赖缺失')
    modules[id](module, module.exports, requireModule)
    return module.exports
  }
  if (!scope.bdms?.init) {
    if (!modules) throw new Error('invalid_response: 豆包安全 SDK 未注册初始化入口')
    requireModule('ISb')
  }
  if (typeof scope.bdms?.init !== 'function')
    throw new Error('invalid_response: 豆包安全 SDK 初始化入口无效')
  scope.bdms.init(input.config)
  return {
    storage,
    session_storage: sessionStorageValues,
    tick: transport.tick,
    complete: transport.complete,
    finish: transport.finish,
    sign(request) {
      transport.prepare(request.url)
      scope.fetch(request.url, request)
      transport.tick()
      const signed = transport.signed()
      const pathname = address(request.url).pathname
      const required =
        input.config.paths.include.some((path) => pathname.startsWith(path)) &&
        !input.config.paths.exclude.includes(pathname)
      if (required && !/[?&]a_bogus=/.test(signed.url)) throw new Error('安全 SDK 未完成签名')
      return signed
    },
  }
}

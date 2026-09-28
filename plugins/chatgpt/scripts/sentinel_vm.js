// ChatGPT 会在 turnstile.dx 中下发一段小型 VM 程序。这里实现它公开在网页脚本中的通用
// 指令语义，并提供最小浏览器环境；不启动 Chromium，也不把任何一次性令牌写进日志。
function chatgptApiSolveTurnstile(dx, requirementsToken, auth) {
  if (!dx) return ''
  const vm = chatgptApiCreateVm(requirementsToken, auth)
  const result = vm.executeInstructions(vm.decodeProgram(dx, requirementsToken))
  if (!result) {
    const strings = Object.entries(vm.memory)
      .filter(([, value]) => typeof value === 'string')
      .map(([key, value]) => ({ key, length: value.length }))
      .sort((left, right) => right.length - left.length)
    throw new Error(
      'Sentinel VM 没有返回 turnstile token，已执行 ' +
        vm.stepCount +
        ' 条，最近指令 ' +
        vm.recentOpcodes.join(',') +
        '，最长字符串 ' +
        JSON.stringify(strings.slice(0, 5))
    )
  }
  return result
}

const CHATGPT_VM_TOKEN_KEY = 16

function chatgptApiCreateVm(requirementsToken, auth) {
  const vm = {
    memory: {},
    frames: [],
    environment: chatgptApiVmEnvironment(auth),
    operations: {},
    stepCount: 0,
    recentOpcodes: [],
  }
  vm.currentFrame = () => vm.frames[vm.frames.length - 1]
  vm.setValue = (key, value) => {
    vm.memory[key] = value
    return value
  }
  vm.decodeProgram = (encoded, key) => {
    const decoded = chatgptApiBase64DecodeBinary(encoded)
    return JSON.parse(chatgptApiXor(decoded, String(key || '')))
  }
  vm.executeInstructions = (instructions) => chatgptApiExecuteVmInstructions(vm, instructions)
  vm.executeEncoded = (encoded) =>
    vm.executeInstructions(vm.decodeProgram(encoded, vm.memory[CHATGPT_VM_TOKEN_KEY]))
  chatgptApiInstallVmDataOperations(vm)
  chatgptApiInstallVmControlOperations(vm)
  chatgptApiInstallVmUtilityOperations(vm)
  // 网页实现把每个指令处理器也放进同一个寄存器表，VM 内部会通过 CALL 指令再次调用它们。
  for (const [opcode, operation] of Object.entries(vm.operations)) vm.memory[opcode] = operation
  // 3/4 号寄存器在执行阶段会被替换成“完成/失败”回调，CALL 会传入已经解析出的值。
  vm.memory[3] = (value) => {
    const frame = vm.currentFrame()
    frame.result = chatgptApiBase64EncodeBinary(String(value))
    frame.halted = true
    return frame.result
  }
  vm.memory[4] = (value) => {
    const frame = vm.currentFrame()
    frame.error = new Error(chatgptApiBase64EncodeBinary(String(value)))
    frame.halted = true
  }
  // 指令 10 和 16 是 VM 的预置寄存器，而不是可执行函数。
  vm.memory[10] = vm.environment
  vm.memory[CHATGPT_VM_TOKEN_KEY] = requirementsToken
  return vm
}

function chatgptApiExecuteVmInstructions(vm, instructions) {
  const frame = { halted: false, result: '', error: null }
  const previousQueue = vm.memory[9]
  vm.memory[9] = [...(Array.isArray(instructions) ? instructions : [])]
  vm.frames.push(frame)
  try {
    // 指令可以直接替换 9 号队列寄存器以进入下一层字节码，必须每轮重新读取它。
    while (Array.isArray(vm.memory[9]) && vm.memory[9].length > 0) {
      const instruction = vm.memory[9].shift()
      if (frame.halted) break
      if (++vm.stepCount > 100000) throw new Error('Sentinel VM 指令数超过安全限制')
      const opcode = Number(instruction?.[0])
      vm.recentOpcodes.push(opcode)
      if (vm.recentOpcodes.length > 16) vm.recentOpcodes.shift()
      // VM 会先用 COPY 把基础指令映射到随机浮点寄存器，后续 opcode 因此不能只查固定表。
      const operation = vm.memory[opcode]
      if (typeof operation !== 'function') {
        throw new Error(
          'Sentinel VM 未知指令: ' +
            opcode +
            '，层级 ' +
            vm.frames.length +
            '，最近指令 ' +
            vm.recentOpcodes.join(',')
        )
      }
      operation(...instruction.slice(1))
    }
    if (frame.error) throw frame.error
    return frame.result
  } finally {
    vm.frames.pop()
    vm.memory[9] = previousQueue
  }
}

function chatgptApiInstallVmDataOperations(vm) {
  const { memory, operations, environment } = vm
  operations[0] = (encoded) => vm.executeEncoded(encoded)
  operations[1] = (target, key) =>
    vm.setValue(target, chatgptApiXor(String(memory[target]), String(memory[key])))
  operations[2] = (target, value) => vm.setValue(target, value)
  operations[5] = (target, source) => {
    const current = memory[target]
    if (Array.isArray(current)) current.push(memory[source])
    else vm.setValue(target, current + memory[source])
  }
  operations[6] = (target, objectKey, propertyKey) =>
    vm.setValue(target, memory[objectKey]?.[memory[propertyKey]])
  operations[7] = (callableKey, ...keys) => {
    // 浏览器对象会随版本增加探针；生产版解释器对不存在的方法按 undefined 调用处理。
    if (typeof memory[callableKey] !== 'function') return undefined
    return memory[callableKey](...keys.map((key) => memory[key]))
  }
  operations[8] = (target, source) => vm.setValue(target, memory[source])
  operations[10] = environment
  operations[11] = (target, patternKey) => {
    const sources = environment.document.scripts.map((script) => script.src)
    const match = sources.map((src) => src.match(memory[patternKey])).find(Boolean)?.[0] || null
    vm.setValue(target, match)
  }
  operations[12] = (target) => vm.setValue(target, memory)
  operations[13] = (errorTarget, callableKey, ...args) => {
    try {
      memory[callableKey](...args)
    } catch (error) {
      vm.setValue(errorTarget, String(error))
    }
  }
  operations[14] = (target, source) => vm.setValue(target, JSON.parse(String(memory[source])))
  operations[15] = (target, source) => vm.setValue(target, JSON.stringify(memory[source]))
}

function chatgptApiInstallVmControlOperations(vm) {
  const { memory, operations } = vm
  operations[17] = (target, callableKey, ...keys) => {
    try {
      vm.setValue(target, memory[callableKey](...keys.map((key) => memory[key])))
    } catch (error) {
      vm.setValue(target, String(error))
    }
  }
  operations[18] = (target) =>
    vm.setValue(target, chatgptApiBase64DecodeBinary(String(memory[target])))
  operations[19] = (target) =>
    vm.setValue(target, chatgptApiBase64EncodeBinary(String(memory[target])))
  operations[20] = (left, right, callableKey, ...args) =>
    memory[left] === memory[right] ? memory[callableKey](...args) : null
  operations[21] = (left, right, threshold, callableKey, ...args) =>
    Math.abs(memory[left] - memory[right]) > memory[threshold] ? memory[callableKey](...args) : null
  operations[22] = (errorTarget, instructions) => {
    try {
      vm.executeInstructions(instructions)
    } catch (error) {
      vm.setValue(errorTarget, String(error))
    }
  }
  operations[23] = (definedKey, callableKey, ...args) =>
    memory[definedKey] === undefined ? null : memory[callableKey](...args)
  operations[24] = (target, objectKey, propertyKey) => {
    const object = memory[objectKey]
    const property = memory[propertyKey]
    const method = object?.[property]
    vm.setValue(target, typeof method === 'function' ? method.bind(object) : undefined)
  }
}

function chatgptApiInstallVmUtilityOperations(vm) {
  const { memory, operations } = vm
  operations[25] = () => {}
  operations[26] = () => {}
  operations[27] = (target, source) => {
    const current = memory[target]
    if (Array.isArray(current)) current.splice(current.indexOf(memory[source]), 1)
    else vm.setValue(target, current - memory[source])
  }
  operations[28] = () => {}
  operations[29] = (target, left, right) => vm.setValue(target, memory[left] < memory[right])
  operations[30] = (functionKey, resultKey, parameterKeys, body) => {
    const hasParameters = Array.isArray(body)
    const params = hasParameters ? parameterKeys : []
    const instructions = hasParameters ? body : parameterKeys || []
    vm.setValue(functionKey, (...values) => {
      for (let index = 0; index < params.length; index++) {
        vm.setValue(params[index], values[index])
      }
      vm.executeInstructions(instructions)
      return memory[resultKey]
    })
  }
  operations[33] = (target, left, right) =>
    vm.setValue(target, Number(memory[left]) * Number(memory[right]))
  operations[34] = (target, source) => vm.setValue(target, memory[source])
  operations[35] = (target, left, right) => {
    const divisor = Number(memory[right])
    vm.setValue(target, divisor === 0 ? 0 : Number(memory[left]) / divisor)
  }
}

function chatgptApiVmEnvironment(auth) {
  const fingerprint = chatgptApiFingerprint(auth)
  const scriptSources = chatgptApiVmArray(fingerprint.script_sources)
  const localKeys = chatgptApiVmArray(fingerprint.local_storage_keys)
  const location = chatgptApiVmLocation()
  const localStorage = chatgptApiVmStorage(localKeys)
  const element = chatgptApiVmElement
  const document = chatgptApiVmDocument(fingerprint, scriptSources, element)
  const navigator = chatgptApiVmNavigator(fingerprint)
  const screen = chatgptApiVmScreen(fingerprint)
  const environment = chatgptApiVmGlobals(
    fingerprint,
    document,
    navigator,
    screen,
    location,
    localStorage
  )
  environment.window = environment
  environment.self = environment
  chatgptApiVmApplyMissingKeys(environment, chatgptApiVmValue(fingerprint.window_keys, []))
  return environment
}

function chatgptApiVmLocation() {
  return {
    href: 'https://chatgpt.com/',
    origin: 'https://chatgpt.com',
    protocol: 'https:',
    host: 'chatgpt.com',
    hostname: 'chatgpt.com',
    pathname: '/',
    search: '',
    hash: '',
    toString() {
      return this.href
    },
  }
}

function chatgptApiVmStorage(localKeys) {
  const values = {}
  const storage = {
    length: localKeys.length,
    key(index) {
      return localKeys[index] || null
    },
    getItem(key) {
      return Object.prototype.hasOwnProperty.call(values, key) ? values[key] : null
    },
    setItem(key, value) {
      values[String(key)] = String(value)
      this[String(key)] = String(value)
      this.length = Object.keys(values).length
    },
    removeItem(key) {
      delete values[String(key)]
      delete this[String(key)]
      this.length = Object.keys(values).length
    },
    clear() {
      for (const key of Object.keys(values)) {
        delete values[key]
        delete this[key]
      }
      this.length = 0
    },
  }
  for (const key of localKeys) storage[key] = null
  return storage
}

function chatgptApiVmElement() {
  return {
    style: {},
    hidden: false,
    getBoundingClientRect: () => ({
      x: 0,
      y: 1219,
      width: 37.8125,
      height: 30,
      top: 1219,
      right: 37.8125,
      bottom: 1249,
      left: 0,
    }),
    getContext: () => chatgptApiVmCanvasContext(),
    setAttribute() {},
    appendChild() {},
    removeChild() {},
    remove() {},
  }
}

function chatgptApiVmCanvasContext() {
  const context = {
    canvas: { width: 300, height: 150 },
    fillStyle: '#000000',
    font: '10px sans-serif',
    textBaseline: 'alphabetic',
    getExtension: () => null,
    getSupportedExtensions: () => [],
    getParameter: () => null,
    getShaderPrecisionFormat: () => ({ rangeMin: 127, rangeMax: 127, precision: 23 }),
    measureText: (text) => ({ width: String(text || '').length * 6 }),
    getImageData: () => ({ data: [0, 0, 0, 0], width: 1, height: 1 }),
    toDataURL: () => 'data:image/png;base64,',
    fillRect() {},
    fillText() {},
    clearRect() {},
    beginPath() {},
    closePath() {},
    stroke() {},
    createBuffer: () => ({}),
    bindBuffer() {},
    bufferData() {},
    createProgram: () => ({}),
    createShader: () => ({}),
    shaderSource() {},
    compileShader() {},
    attachShader() {},
    linkProgram() {},
  }
  return context
}

function chatgptApiVmDocument(fingerprint, scriptSources, element) {
  const document = {
    scripts: scriptSources.map((src) => ({ src })),
    cookie: '',
    createElement: element,
    querySelector: () => null,
    getElementById: () => null,
    getElementsByTagName: () => [],
    body: element(),
    head: element(),
    documentElement: {
      getAttribute: (name) => (name === 'data-build' ? fingerprint.page_build || '' : null),
    },
  }
  chatgptApiVmApplyMissingKeys(document, chatgptApiVmValue(fingerprint.document_keys, []))
  return document
}

function chatgptApiVmNavigator(fingerprint) {
  return {
    userAgent: chatgptApiVmValue(fingerprint.user_agent, 'Mozilla/5.0'),
    language: chatgptApiVmValue(fingerprint.language, 'en-US'),
    languages: chatgptApiVmValue(fingerprint.languages, ['en-US']),
    platform: chatgptApiVmValue(fingerprint.platform, 'MacIntel'),
    hardwareConcurrency: chatgptApiVmValue(fingerprint.hardware_concurrency, 8),
    maxTouchPoints: 0,
    vendor: 'Google Inc.',
    webdriver: false,
  }
}

function chatgptApiVmScreen(fingerprint) {
  return {
    width: chatgptApiVmValue(fingerprint.screen_width, 1366),
    height: chatgptApiVmValue(fingerprint.screen_height, 768),
    availWidth: chatgptApiVmValue(fingerprint.screen_width, 1366),
    availHeight: chatgptApiVmValue(fingerprint.screen_height, 768),
  }
}

function chatgptApiVmGlobals(fingerprint, document, navigator, screen, location, localStorage) {
  return {
    window: null,
    self: null,
    document,
    navigator,
    screen,
    location,
    history: { length: chatgptApiVmValue(fingerprint.history_length, 1) },
    localStorage,
    sessionStorage: { length: 0, key: () => null, getItem: () => null },
    performance: {
      now: () => Date.now() % 100000,
      timeOrigin: chatgptApiVmValue(fingerprint.time_origin, Date.now()),
      memory: { jsHeapSizeLimit: 4294705152 },
    },
    innerWidth: chatgptApiVmValue(fingerprint.page_width, 1366),
    innerHeight: chatgptApiVmValue(fingerprint.page_height, 768),
    devicePixelRatio: chatgptApiVmValue(fingerprint.pixel_ratio, 1),
    Object,
    Array,
    JSON,
    Math,
    Date,
    String,
    Number,
    Boolean,
    RegExp,
    Reflect,
    URL: chatgptApiVmUrl,
    URLSearchParams: chatgptApiVmUrlSearchParams,
    btoa: chatgptApiBase64EncodeBinary,
    atob: chatgptApiBase64DecodeBinary,
    getComputedStyle: () => ({}),
    matchMedia: () => ({ matches: false }),
  }
}

function chatgptApiVmApplyMissingKeys(target, keys) {
  for (const key of keys) {
    if (!(key in target)) target[key] = null
  }
}

function chatgptApiVmArray(value) {
  return Array.isArray(value) ? value : []
}

function chatgptApiVmValue(value, fallback) {
  return value || fallback
}

function chatgptApiVmUrl(value) {
  const text = String(value || 'https://chatgpt.com/')
  return {
    href: text,
    origin: 'https://chatgpt.com',
    protocol: 'https:',
    host: 'chatgpt.com',
    hostname: 'chatgpt.com',
    pathname: '/',
    search: '',
    hash: '',
    toString: () => text,
  }
}

function chatgptApiVmUrlSearchParams() {
  return { keys: () => [][Symbol.iterator]() }
}

function chatgptApiXor(value, key) {
  if (!key) return value
  let output = ''
  for (let index = 0; index < value.length; index++) {
    output += String.fromCharCode(value.charCodeAt(index) ^ key.charCodeAt(index % key.length))
  }
  return output
}

function chatgptApiBase64Encode(value) {
  return host.call('crypto.base64_encode_utf8', { text: String(value) }).value
}

function chatgptApiBase64Decode(value) {
  return host.call('crypto.base64url_decode_utf8', { text: String(value) }).value
}

function chatgptApiBase64EncodeBinary(value) {
  return host.call('crypto.base64_encode_latin1', { text: String(value) }).value
}

function chatgptApiBase64DecodeBinary(value) {
  return host.call('crypto.base64_decode_latin1', { text: String(value) }).value
}

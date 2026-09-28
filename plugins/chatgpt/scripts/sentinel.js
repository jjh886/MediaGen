function chatgptApiSentinelHeaders(auth) {
  const requirementsConfig = chatgptApiSentinelConfig(auth)
  const requirementsToken = 'gAAAAAC' + chatgptApiBase64Encode(JSON.stringify(requirementsConfig))
  const prepare = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/sentinel/chat-requirements/prepare', {
      target_route: '/backend-api/sentinel/chat-requirements/prepare',
      headers: { 'content-type': 'application/json' },
      body: { p: requirementsToken },
      timeout_ms: 60000,
    }),
    '准备 ChatGPT 完整性令牌'
  ).body
  if (prepare?.force_login) {
    throw new Error('LOGIN_EXPIRED: ChatGPT 要求重新登录后才能生成。')
  }
  if (!prepare?.prepare_token) {
    throw new Error('ChatGPT 完整性令牌响应缺少 prepare_token')
  }

  const proof = prepare?.proofofwork?.required
    ? chatgptApiSolveProofOfWork(prepare.proofofwork.seed, prepare.proofofwork.difficulty, auth)
    : ''
  const turnstile = prepare?.turnstile?.required
    ? chatgptApiSolveTurnstile(prepare.turnstile.dx, requirementsToken, auth)
    : ''
  const sessionObserver = chatgptApiPrepareSessionObserver(prepare, requirementsToken, auth)
  const finalize = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/sentinel/chat-requirements/finalize', {
      target_route: '/backend-api/sentinel/chat-requirements/finalize',
      headers: { 'content-type': 'application/json' },
      body: chatgptApiCompactObject({
        prepare_token: prepare?.prepare_token || '',
        proofofwork: proof,
        turnstile,
      }),
      timeout_ms: 60000,
    }),
    '完成 ChatGPT 完整性令牌'
  ).body
  if (!finalize?.token) throw new Error('ChatGPT 完整性令牌响应缺少 token')
  const sessionObserverToken = chatgptApiFinalizeSessionObserver(sessionObserver, auth)

  return chatgptApiCompactObject({
    'openai-sentinel-chat-requirements-token': finalize.token,
    'openai-sentinel-chat-requirements-prepare-token': prepare.prepare_token,
    'openai-sentinel-proof-token': proof,
    'openai-sentinel-so-token': sessionObserverToken,
    'openai-sentinel-turnstile-token': turnstile,
    'oai-telemetry': '[1,null]',
  })
}

function chatgptApiPrepareSessionObserver(prepare, requirementsToken, auth) {
  const segment = prepare?.so
  if (!segment?.required) return null
  if (!segment.collector_dx || !segment.snapshot_dx) {
    throw new Error('ChatGPT Session Observer 响应缺少 collector_dx 或 snapshot_dx')
  }
  const vm = chatgptApiCreateVm(requirementsToken, auth)
  vm.executeInstructions(vm.decodeProgram(segment.collector_dx, requirementsToken))
  return {
    vm,
    snapshot_dx: segment.snapshot_dx,
    chat_token: prepare?.token || prepare?.prepare_token || '',
    requirements_token: requirementsToken,
  }
}

function chatgptApiFinalizeSessionObserver(session, auth) {
  if (!session) return ''
  const result = session.vm.executeInstructions(
    session.vm.decodeProgram(session.snapshot_dx, session.requirements_token)
  )
  if (!result) throw new Error('ChatGPT Session Observer 没有返回 token')
  const deviceId = chatgptApiCookieValue(auth, 'oai-did')
  if (!deviceId || !session.chat_token) {
    throw new Error('ChatGPT Session Observer 缺少设备 ID 或 chat token')
  }
  return chatgptApiBase64Encode(
    JSON.stringify({
      so: result,
      c: session.chat_token,
      id: deviceId,
      flow: deviceId,
    })
  )
}

function chatgptApiSentinelConfig(auth) {
  const fingerprint = chatgptApiFingerprint(auth)
  const start = Date.now()
  const config = [
    Number(chatgptApiValueOr(fingerprint.screen_width, 1366)) +
      Number(chatgptApiValueOr(fingerprint.screen_height, 768)),
    String(new Date()),
    4294705152,
    1,
    chatgptApiValueOr(fingerprint.user_agent, 'Mozilla/5.0'),
    chatgptApiRandomOr(fingerprint.script_sources, null),
    chatgptApiValueOr(fingerprint.page_build, ''),
    chatgptApiValueOr(fingerprint.language, 'en-US'),
    chatgptApiValueOr(fingerprint.languages, ['en-US']).join(','),
    0,
    chatgptApiNavigatorSignature(fingerprint),
    chatgptApiRandomOr(fingerprint.document_keys, 'location'),
    chatgptApiRandomOr(fingerprint.window_keys, 'navigator'),
    Math.random() * 1000,
    chatgptApiSessionId(),
    '',
    Number(chatgptApiValueOr(fingerprint.hardware_concurrency, 8)),
    Number(chatgptApiValueOr(fingerprint.time_origin, Date.now())),
    0,
    0,
    0,
    0,
    0,
    0,
    0,
  ]
  config[9] = Date.now() - start
  return config
}

function chatgptApiProofConfig(auth) {
  const config = chatgptApiSentinelConfig(auth)
  config[3] = Math.random()
  config[9] = Math.random() * 10
  return config
}

function chatgptApiSolveProofOfWork(seed, difficulty, auth) {
  const started = Date.now()
  const config = chatgptApiProofConfig(auth)
  for (let attempt = 0; attempt < 500000; attempt++) {
    config[3] = attempt
    config[9] = Date.now() - started
    const encoded = chatgptApiBase64Encode(JSON.stringify(config))
    if (
      chatgptApiProofHash(String(seed) + encoded).slice(0, String(difficulty).length) <= difficulty
    ) {
      return 'gAAAAAB' + encoded + '~S'
    }
  }
  throw new Error('ChatGPT proof-of-work 在 500000 次内没有找到答案')
}

function chatgptApiProofHash(value) {
  let hash = 2166136261
  for (let index = 0; index < value.length; index++) {
    hash ^= value.charCodeAt(index)
    hash = Math.imul(hash, 16777619) >>> 0
  }
  hash ^= hash >>> 16
  hash = Math.imul(hash, 2246822507) >>> 0
  hash ^= hash >>> 13
  hash = Math.imul(hash, 3266489909) >>> 0
  hash ^= hash >>> 16
  return (hash >>> 0).toString(16).padStart(8, '0')
}

function chatgptApiRandomItem(values) {
  return values[Math.floor(Math.random() * values.length)]
}

// Sentinel 使用浏览器里的 JavaScript 真值回退规则，集中封装可避免配置项重复判断。
function chatgptApiValueOr(value, fallback) {
  return value || fallback
}

function chatgptApiRandomOr(values, fallback) {
  return chatgptApiRandomItem(values || []) || fallback
}

function chatgptApiNavigatorSignature(fingerprint) {
  const key = chatgptApiRandomOr(fingerprint.navigator_keys, 'webkitGetUserMedia')
  return key + '−function ' + key + '() { [native code] }'
}

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import vm from 'node:vm'
import { createHash } from 'node:crypto'

function load(scripts, host) {
  const context = vm.createContext({ host })
  for (const script of scripts)
    vm.runInContext(readFileSync(`plugins/grok/scripts/${script}.js`, 'utf8'), context)
  return context
}
function fixture(respond) {
  const files = new Map([
    [
      '/profile/auth.json',
      {
        access_token: 'test-only-token',
        expires_at: Date.now() + 3600000,
        agent_id: '11111111-1111-4111-8111-111111111111',
        user_id: 'test-user',
      },
    ],
  ])
  const requests = []
  let checkpoint = {}
  return {
    files,
    requests,
    call(action, params) {
      if (action === 'crypto.sha256_hex')
        return { value: createHash('sha256').update(params.text).digest('hex') }
      if (action === 'file.read_json') return files.get(params.path) || {}
      if (action === 'file.write_json') return files.set(params.path, params.value)
      if (action.startsWith('lock.')) return {}
      if (action === 'crypto.random_uuid') return { value: '11111111-1111-4111-8111-111111111111' }
      if (action === 'crypto.base64url_decode_utf8')
        return { value: Buffer.from(params.text, 'base64url').toString() }
      requests.push({ action, ...params })
      return respond(action, params)
    },
    checkpoint(value) {
      if (value) checkpoint = value
      return checkpoint
    },
    sleep() {},
  }
}

test('Grok 登录等待、退避、令牌保存与重复完成保持幂等', () => {
  let count = 0
  const token = Buffer.from(JSON.stringify({ sub: 'user', email: 'test@example.test' })).toString(
    'base64url'
  )
  const host = fixture(() =>
    ++count === 1
      ? { ok: false, status: 400, body: { error: 'slow_down' } }
      : {
          ok: true,
          status: 200,
          body: { access_token: `x.${token}.x`, refresh_token: 'refresh', expires_in: 3600 },
        }
  )
  host.files.set('/profile/login.json', {
    device_code: 'device',
    interval: 5,
    expires_at: Date.now() + 60000,
  })
  const script = load(['finish_login'], host)
  assert.equal(script.main({ profile_dir: '/profile' }).loggedIn, false)
  assert.equal(host.files.get('/profile/login.json').interval, 10)
  assert.equal(script.main({ profile_dir: '/profile' }).loggedIn, false)
  assert.equal(count, 1)
  host.files.get('/profile/login.json').next_poll_at = 0
  assert.equal(script.main({ profile_dir: '/profile' }).userId, 'user')
  assert.equal(script.main({ profile_dir: '/profile' }).userId, 'user')
  assert.equal(count, 2)
  assert.equal(host.files.get('/profile/auth.json').refresh_token, 'refresh')
  assert.equal(Object.keys(host.files.get('/profile/login.json')).length, 0)
})

test('Grok 刷新令牌沿用轮换、持久化并释放锁', () => {
  const host = fixture(() => ({
    ok: true,
    status: 200,
    body: { access_token: 'fresh', refresh_token: 'rotated', expires_in: 600 },
  }))
  host.files.get('/profile/auth.json').expires_at = 0
  host.files.get('/profile/auth.json').refresh_token = 'old'
  const script = load(['auth'], host)
  assert.equal(script.grokAuth({ profile_dir: '/profile' }).access_token, 'fresh')
  assert.equal(host.requests.length, 1)
  assert.equal(host.requests[0].body.refresh_token, 'old')
  assert.equal(host.files.get('/profile/auth.json').refresh_token, 'rotated')
})

test('Grok 请求标识对应公共标识，缓存会话仅在指定稳定键时生成', () => {
  const host = fixture(() => {
    throw new Error('unexpected HTTP')
  })
  const script = load(['auth'], host)
  const auth = host.files.get('/profile/auth.json')
  const ctx = {
    request_id: 'req_0123456789abcdef0123456789abcdef',
    request: { model: 'grok-4.7', prompt_cache_key: 'my-session' },
  }
  const headers = script.grokHeaders(ctx, auth, true)
  assert.equal(headers['x-grok-client-version'], '1.0.13')
  assert.equal(headers['user-agent'], 'xai-grok-workspace/1.0.13')
  assert.equal(headers['x-grok-client-mode'], 'interactive')
  assert.equal(headers['x-grok-req-id'], '01234567-89ab-cdef-0123-456789abcdef')
  assert.equal(headers['x-client-request-id'], ctx.request_id)
  assert.equal(
    headers['x-grok-session-id'],
    script.grokHeaders(ctx, auth, true)['x-grok-session-id']
  )
  assert.match(
    headers['x-grok-session-id'],
    /^[0-9a-f]{8}-[0-9a-f]{4}-8[0-9a-f]{3}-a[0-9a-f]{3}-[0-9a-f]{12}$/
  )
  assert.equal(script.grokHeaders({ request: {} }, auth, true)['x-grok-session-id'], undefined)
})

test('Grok 协议版本过期可诊断且不暴露上游敏感正文', () => {
  const script = load(['auth'], {})
  assert.throws(
    () => script.grokError({ status: 426, body: { error: 'private-upstream-token' } }),
    (error) =>
      error.message.includes('upstream_client_outdated:') &&
      !error.message.includes('private-upstream-token') &&
      !error.message.includes('LOGIN_EXPIRED')
  )
})

test('Grok 文本保留工具、结构化输出、增量和 incomplete 终态', () => {
  const host = fixture(() => {
    throw new Error('unexpected HTTP')
  })
  host.stream = (action, params, callback) => {
    assert.equal(action, 'http.sse_request')
    assert.equal(params.body.store, false)
    assert.equal(params.body.metadata, undefined)
    assert.equal(params.body.tools[0].name, 'weather')
    assert.equal(params.body.text.format.type, 'json_object')
    assert.equal(params.headers['x-client-request-id'], 'request-test')
    assert.match(params.headers['x-grok-req-id'], /^[0-9a-f-]{36}$/)
    assert.equal(params.headers['x-grok-session-id'], undefined)
    callback({
      type: 'response.output_item.done',
      output_index: 0,
      item: { type: 'function_call', name: 'weather', arguments: '{}' },
    })
    callback({
      type: 'response.incomplete',
      response: {
        status: 'incomplete',
        output: [],
        incomplete_details: { reason: 'max_output_tokens' },
      },
    })
    return { ok: true, status: 200 }
  }
  const script = load(['auth', 'text', 'generate_text'], host)
  const result = script.main({
    profile_dir: '/profile',
    request_id: 'request-test',
    request: {
      model: 'grok-4.7',
      input: 'hello',
      metadata: { test: 'local-only' },
      tools: [{ type: 'function', name: 'weather' }],
      text: { format: { type: 'json_object' } },
    },
  })
  assert.equal(result.response.status, 'incomplete')
  assert.equal(result.response.output[0].name, 'weather')
  host.checkpoint({}) // 后续是另一请求，使用独立检查点。
  host.stream = () => ({ ok: true, status: 200 })
  assert.throws(
    () => script.main({ profile_dir: '/profile', request: { model: 'grok-4.7', input: 'hello' } }),
    /未正常完成/
  )
})

test('Grok 多轮历史保留文本、图片和文件，清除上游不接受的输出标识', () => {
  const script = load(['text'], {})
  const item = script.grokInputItem({
    type: 'message',
    role: 'assistant',
    id: 'msg_x',
    status: 'completed',
    content: [
      { type: 'output_text', text: 'first' },
      { type: 'output_text', text: 'second' },
    ],
  })
  assert.equal(item.content, 'first\nsecond')
  assert.equal(item.id, undefined)
  const input = script.grokInputItem({
    role: 'user',
    content: [
      { type: 'input_image', image_url: 'data:image/png;base64,YQ==' },
      { type: 'input_file', filename: 'a.pdf', file_data: 'data:application/pdf;base64,YQ==' },
    ],
  })
  assert.equal(input.content[0].detail, 'auto')
  assert.equal(input.content[1].filename, 'a.pdf')
  assert.ok(script.grokTextCapabilities('grok-4.7').input_modalities.includes('file'))
})

test('Grok 图片编辑保留参考图并把上游 URL 转换成内容', () => {
  const host = fixture((_action, params) =>
    params.method === 'GET'
      ? { ok: true, body_base64: 'aW1hZ2U=' }
      : { ok: true, body: { data: [{ url: 'https://assets.grok.com/image.png' }] } }
  )
  const script = load(['auth', 'generate_image'], host)
  const result = script.main({
    profile_dir: '/profile',
    model_id: 'grok-imagine-image-2.0',
    prompt: 'edit',
    ref_image_dataurls: ['data:image/png;base64,aA=='],
  })
  assert.equal(host.requests[0].url, 'https://api.x.ai/v1/images/edits')
  assert.equal(host.requests[0].body.image.url, 'data:image/png;base64,aA==')
  assert.equal(host.requests[1].headers, undefined)
  assert.equal(result.images[0].url, 'data:image/png;base64,aW1hZ2U=')
  assert.throws(() => script.grokAsset('https://assets.grok.com.evil.test/a'), /无效/)
  assert.throws(() => script.grokAsset('http://127.0.0.1/a'), /无效/)
})

test('Grok 语音以真实编码返回，转写使用 multipart 不伪装翻译', () => {
  const host = fixture((action, params) => {
    if (action === 'audio.transcode') return { base64: 'UklGRg==', mime_type: 'audio/wav' }
    if (params.url.endsWith('/stt')) return { ok: true, body: { text: '你好' } }
    return { ok: true, body_base64: 'SUQz' }
  })
  const speech = load(['auth', 'generate_audio'], host)
  const result = speech.main({
    profile_dir: '/profile',
    request: { input: '你好', voice: 'eve', response_format: 'wav' },
  })
  assert.equal(result.mime_type, 'audio/wav')
  assert.equal(result.audio_base64, 'UklGRg==')
  assert.equal(host.requests[0].body.output_format.codec, 'mp3')
  host.checkpoint({})
  assert.throws(() => speech.main({ request: { speed: 2 } }), /speed/)
  const stt = load(['auth', 'transcribe_audio'], host)
  assert.equal(
    stt.main({ profile_dir: '/profile', request: { model: 'grok-stt', file: { base64: 'YQ==' } } })
      .text,
    '你好'
  )
  assert.equal(host.requests.at(-1).body_format, 'multipart')
  assert.equal(host.requests.at(-1).headers['content-type'], undefined)
})

test('Grok 视频恢复检查点不会再次创建任务', () => {
  const host = fixture(() => ({
    ok: true,
    body: { status: 'completed', video: { url: 'https://videos.x.ai/test.mp4' } },
  }))
  host.checkpoint({ phase: 'polling', upstream_job_id: 'existing' })
  const script = load(['auth', 'generate_video'], host)
  assert.equal(script.main({ profile_dir: '/profile', seconds: '6' }).videos.length, 1)
  assert.equal(host.requests.length, 1)
  assert.equal(host.requests[0].method, 'GET')
  assert.ok(host.requests[0].url.endsWith('/videos/existing'))
})

test('实时语音使用 OAuth 主 WebSocket，平台差异留在插件', () => {
  const host = fixture(() => {
    throw new Error('unexpected HTTP')
  })
  const grok = load(['auth', 'connect_realtime'], host)
  const result = grok.main({ profile_dir: '/profile', request: { model: 'grok-voice-latest' } })
  assert.equal(result.connection.url, 'wss://api.x.ai/v1/realtime?model=grok-voice-latest')
  assert.equal(
    result.client_event_adapter.events['session.update'].move['/session/audio/output/voice'],
    '/session/voice'
  )
  const codex = vm.createContext({
    codexEnsureFreshAuth: () => ({ tokens: { access_token: 'token' } }),
    codexAccessToken: (auth) => auth.tokens.access_token,
  })
  vm.runInContext(readFileSync('plugins/codex/scripts/connect_realtime.js', 'utf8'), codex)
  assert.ok(
    codex
      .main({ request: { model: 'gpt-realtime-2.1' } })
      .connection.url.startsWith('wss://api.openai.com/v1/realtime')
  )
})

test('Grok 额度分别解析周/月百分比和免费响应头，缺失数据不能编造', () => {
  const ctx = load(['quota'], {})
  const windows = ctx.grokBillingWindows(
    {
      config: {
        creditUsagePercent: 25,
        currentPeriod: { end: '2026-09-28T00:00:00Z' },
        productUsage: [{ product: 'build', usagePercent: 30 }],
      },
    },
    {
      config: {
        monthlyLimit: { val: 15000 },
        used: { val: 3000 },
        billingPeriodEnd: '2026-10-01T00:00:00Z',
      },
    }
  )
  assert.deepEqual(
    Array.from(windows, (w) => w.remaining_percent),
    [75, 70, 80]
  )
  assert.equal(windows[0].reset_at, '2026-09-28T00:00:00.000Z')
  assert.equal(
    ctx.grokBillingWindows({ config: {} }, { config: { monthlyLimit: 0, used: 0 } }).length,
    0
  )
  const header = ctx.grokHeaderWindows({
    'X-RateLimit-Limit-Requests': '21',
    'X-RateLimit-Remaining-Requests': '0',
    'X-RateLimit-Reset-Requests': '60',
    'x-ratelimit-limit-tokens': '1000000',
    'x-ratelimit-remaining-tokens': '250000',
  })
  assert.equal(header[0].remaining_count, 0)
  assert.equal(header[0].remaining_percent, 0)
  assert.ok(Date.parse(header[0].reset_at) > Date.now())
  assert.equal(header[1].remaining_percent, 25)
  assert.equal(header[1].unit, 'Token')
  assert.equal(ctx.grokHeaderWindows({}).length, 0)
})

test('Grok 主动额度刷新在免费 billing 无窗口时读取响应头，自动保活不消耗生成额度', () => {
  const host = fixture((_action, params) =>
    params.url.includes('/responses')
      ? { status: 200, ok: true, headers: { 'x-ratelimit-remaining-tokens': '800' }, body: {} }
      : { status: 200, ok: true, body: { config: {} } }
  )
  const ctx = load(['auth', 'quota', 'check_account'], host)
  const silent = ctx.main({ profile_dir: '/profile' })
  assert.equal(silent.status, 'available')
  assert.equal(
    host.requests.some((request) => request.url.includes('/responses')),
    false
  )
  const manual = ctx.main({ profile_dir: '/profile', refresh_quota: true })
  assert.equal(manual.quota_windows[0].remaining_count, 800)
})

test('Grok 模型额度为零与临时限流分开，不能停用整账号或无限重试', () => {
  const ctx = load(
    ['auth'],
    fixture(() => ({}))
  )
  assert.throws(
    () =>
      ctx.grokError({
        status: 429,
        body: {
          error:
            'Requests per Minute (actual/limit): 0/0. Your rate limit tier is determined based on historical API spend.',
        },
      }),
    /insufficient_quota/
  )
  assert.throws(
    () =>
      ctx.grokError({ status: 429, body: { error: 'Requests per Minute (actual/limit): 11/10.' } }),
    /rate_limit_exceeded/
  )
  assert.throws(() => ctx.grokError({ status: 403, body: {} }), /account_permission_denied/)
  assert.throws(
    () =>
      ctx.grokError({
        status: 400,
        body: { error: 'File content is currently unsupported for ZDR customers.' },
      }),
    /account_permission_denied/
  )
  const manifest = JSON.parse(readFileSync('plugins/grok/manifest.json', 'utf8'))
  const rule = manifest.failure_strategy.find((rule) => rule.category === 'insufficient_quota')
  assert.equal(rule.retry_action, 'fail')
  assert.equal(rule.account_state, undefined)
})

test('手动额度刷新会更新尚未到期的 Grok 授权，普通保活继续复用有效令牌', () => {
  const host = fixture(() => ({
    ok: true,
    status: 200,
    body: { access_token: 'fresh-membership', refresh_token: 'rotated-refresh', expires_in: 3600 },
  }))
  host.files.get('/profile/auth.json').refresh_token = 'old-refresh'
  const ctx = load(['auth'], host)
  assert.equal(ctx.grokAuth({ profile_dir: '/profile' }).access_token, 'test-only-token')
  assert.equal(host.requests.length, 0)
  assert.equal(
    ctx.grokAuth({ profile_dir: '/profile', refresh_quota: true }).access_token,
    'fresh-membership'
  )
  assert.equal(host.requests[0].url, 'https://auth.x.ai/oauth2/token')
  assert.equal(host.files.get('/profile/auth.json').refresh_token, 'rotated-refresh')
})

test('Grok 订阅显示读取新授权的真实等级，不能根据更高速率误判 Heavy', () => {
  const ctx = load(
    ['auth', 'quota'],
    fixture(() => ({}))
  )
  const token = (claims) =>
    'header.' + Buffer.from(JSON.stringify(claims)).toString('base64url') + '.signature'
  assert.equal(ctx.grokSubscriptionTier({ access_token: token({ tier: 1 }) }), 'supergrok')
  assert.equal(ctx.grokSubscriptionTier({ access_token: token({ tier: 5 }) }), 'supergrok_heavy')
  assert.equal(ctx.grokSubscriptionTier({ access_token: token({ tier: 0 }) }), 'free')
  assert.equal(ctx.grokSubscriptionTier({ access_token: token({ tier: null }) }), '')
  assert.equal(ctx.grokSubscriptionTier({ access_token: token({ tier: 99 }) }), '')
  assert.equal(ctx.grokSubscriptionTier({ access_token: 'invalid' }), '')
})

test('Grok 4.7 动态目录保留上游档位、默认值、上下文与搜索能力', () => {
  const catalog = {
    id: 'grok-4.7',
    name: 'Grok 4.7',
    context_window: 500000,
    max_completion_tokens: 1000000,
    supports_backend_search: true,
    reasoning_effort: 'low',
    reasoning_efforts: [
      { value: 'xhigh' },
      { value: 'high', default: true },
      { value: 'medium' },
      { value: 'low' },
      { value: 'low' },
      { value: 'invalid' },
    ],
  }
  const host = fixture(() => ({
    ok: true,
    body: {
      data: [
        catalog,
        { id: 'grok-imagine-image' },
        { id: 'grok-4.5' },
        { id: ' grok-4.6 ' },
        { id: 'grok-build-0.1' },
        null,
      ],
    },
  }))
  const script = load(['auth', 'text', 'list_models'], host)
  const models = script.main({ profile_dir: '/profile' }).models
  assert.equal(models.length, 1)
  const caps = models[0].capabilities
  assert.equal(models[0].display_name, 'Grok 4.7')
  assert.deepEqual(Array.from(caps.reasoning.efforts), ['xhigh', 'high', 'medium', 'low'])
  assert.equal(caps.reasoning.default_effort, 'high')
  assert.equal(caps.context_window, 500000)
  assert.equal(caps.max_output_tokens, 1000000)
  assert.equal(caps.supports_backend_search, true)
  const changed = script.grokTextCapabilities('grok-4.7', {
    reasoning_efforts: [{ value: 'low' }],
    reasoning_effort: 'high',
    context_window: 300000,
    supports_backend_search: false,
  })
  assert.equal(changed.context_window, 300000)
  assert.equal(changed.reasoning.default_effort, undefined)
  assert.equal(changed.supports_backend_search, false)
  const disabled = script.grokTextCapabilities('future-model', { supports_reasoning_effort: false })
  assert.deepEqual(Array.from(disabled.reasoning.efforts), ['none'])
})

test('Grok 4.7 文本请求保留模型和 xhigh 思考参数', () => {
  const host = fixture(() => {
    throw new Error('unexpected HTTP')
  })
  host.stream = (_action, params, callback) => {
    assert.equal(params.body.model, 'grok-4.7')
    assert.equal(params.body.reasoning.effort, 'xhigh')
    callback({
      type: 'response.completed',
      response: { model: params.body.model, status: 'completed', output: [] },
    })
    return { ok: true, status: 200 }
  }
  const script = load(['auth', 'text', 'generate_text'], host)
  assert.equal(
    script.main({
      profile_dir: '/profile',
      request: { model: 'grok-4.7', input: '你好', reasoning: { effort: 'xhigh' } },
    }).response.model,
    'grok-4.7'
  )
})

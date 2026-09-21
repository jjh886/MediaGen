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
    request: { model: 'grok-4.6', prompt_cache_key: 'my-session' },
  }
  const headers = script.grokHeaders(ctx, auth, true)
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

test('Grok 文本保留工具、结构化输出、增量和 incomplete 终态', () => {
  const host = fixture(() => {
    throw new Error('unexpected HTTP')
  })
  host.stream = (action, params, callback) => {
    assert.equal(action, 'http.sse_request')
    assert.equal(params.body.store, false)
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
      model: 'grok-4.6',
      input: 'hello',
      tools: [{ type: 'function', name: 'weather' }],
      text: { format: { type: 'json_object' } },
    },
  })
  assert.equal(result.response.status, 'incomplete')
  assert.equal(result.response.output[0].name, 'weather')
  host.stream = () => ({ ok: true, status: 200 })
  assert.throws(
    () => script.main({ profile_dir: '/profile', request: { model: 'grok-4.6', input: 'hello' } }),
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
  assert.ok(script.grokTextCapabilities('grok-4.6').input_modalities.includes('file'))
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
    model_id: 'grok-imagine-image',
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

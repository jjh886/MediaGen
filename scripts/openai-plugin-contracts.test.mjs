import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import vm from 'node:vm'

function context(platform, scripts, host = {}) {
  const ctx = vm.createContext({ host })
  for (const script of scripts) {
    vm.runInContext(readFileSync(`plugins/${platform}/scripts/${script}`, 'utf8'), ctx, {
      filename: script,
    })
  }
  return ctx
}

for (const platform of ['codex', 'chatgpt-api']) {
  test(`${platform} 所有声明流程可以独立编译`, () => {
    const manifest = JSON.parse(readFileSync(`plugins/${platform}/manifest.json`, 'utf8'))
    for (const [name, flow] of Object.entries(manifest.flows)) {
      for (const step of flow.steps) {
        const files = step.script_files || (step.script_file ? [step.script_file] : [])
        const source = files
          .map((file) =>
            readFileSync(
              file.startsWith('@shared/')
                ? `plugins/_shared/${file.slice(8)}`
                : `plugins/${platform}/scripts/${file}`,
              'utf8'
            )
          )
          .join('\n;\n')
        new vm.Script(source, { filename: name })
      }
    }
  })
}

test('Codex 仅在实际 HTTP 成功时认为认证成功', () => {
  const ctx = context('codex', ['auth.js'])
  assert.equal(ctx.codexHttpSucceeded({ success: true, data: { ok: false, status: 401 } }), false)
  assert.equal(ctx.codexHttpSucceeded({ success: true, data: { ok: true, status: 200 } }), true)
})

test('Codex 输出工具参数、增量与 incomplete 终态不丢失', () => {
  const events = [
    { type: 'response.output_text.delta', delta: '你好' },
    {
      type: 'response.output_item.done',
      output_index: 0,
      item: { id: 'msg_a', type: 'message', content: [{ type: 'output_text', text: '你好' }] },
    },
    {
      type: 'response.incomplete',
      response: {
        id: 'resp_a',
        status: 'incomplete',
        output: [],
        incomplete_details: { reason: 'max_output_tokens' },
      },
    },
  ]
  const ctx = context('codex', ['auth.js', 'text_responses.js'], {
    stream(action, request, callback) {
      assert.equal(action, 'http.sse_request')
      assert.equal(request.fingerprint, 'none')
      assert.equal(request.headers.originator, 'codex-tui')
      events.forEach(callback)
      return { ok: true, status: 200 }
    },
  })
  const result = ctx.codexExecuteResponses({ tokens: { access_token: 'test' } }, {})
  assert.equal(result.response.status, 'incomplete')
  assert.equal(result.response.output[0].content[0].text, '你好')
  assert.equal(result.error, '')
})

test('TTS 不把 AAC 冒充 WAV、PCM 或 FLAC', () => {
  const ctx = context('chatgpt-api', ['audio_speech.js'])
  for (const format of ['wav', 'pcm', 'flac'])
    assert.throws(() => ctx.chatgptApiSpeechFormat(format), /unsupported_format/)
  assert.equal(ctx.chatgptApiSpeechFormat('mp3').actual_format, 'mp3')
  assert.throws(() => ctx.chatgptApiSpeechVoice('unknown'), /unsupported_voice/)
})

test('TTS 仅匹配完整相同的朗读文本', () => {
  const ctx = context('chatgpt-api', ['audio_speech.js'])
  const input = '确切文本'
  const events = [
    {
      conversation_id: 'conversation',
      message: { id: 'wrong', author: { role: 'assistant' }, content: { parts: ['改写文本'] } },
    },
  ]
  assert.equal(ctx.chatgptApiSpeechMessageIds(events, input).message_id, '')
  events.push({
    message: { id: 'correct', author: { role: 'assistant' }, content: { parts: [input] } },
  })
  assert.equal(ctx.chatgptApiSpeechMessageIds(events, input).message_id, 'correct')
})

test('Codex 图片请求保留所选模型并区分原生编辑接口', () => {
  const ctx = context('codex', ['direct_images.js'])
  const result = ctx.codexDirectImageRequest({
    tools: [{ type: 'image_generation', model: 'gpt-image-2.5-flare' }],
    input: [
      {
        content: [
          { type: 'input_image', image_url: 'data:image/png;base64,YQ==' },
          { type: 'input_text', text: '保留原样' },
        ],
      },
    ],
  })
  assert.match(result.url, /images\/edits$/)
  assert.equal(result.body.model, 'gpt-image-2.5-flare')
  assert.equal(result.body.prompt, '保留原样')
  assert.equal(
    ctx.codexCanFallbackImageEndpoint(
      { status: 404, body: { error: { code: 'model_not_found' } } },
      {}
    ),
    false
  )
})

test('文本流的 UTF-8 内容与工具参数不被重复拼接', () => {
  const emitted = []
  const ctx = context('chatgpt-api', ['conversation_results.js', 'text_responses.js'], {
    emit(event) {
      emitted.push(structuredClone(event))
    },
  })
  const writer = ctx.chatgptApiCreateResponseWriter({
    model: 'auto',
    tools: [],
    tool_choice: 'none',
  })
  ctx.chatgptApiStartResponse(writer)
  ctx.chatgptApiFeedResponseText(writer, '你好')
  const response = ctx.chatgptApiCompleteResponse(writer)
  assert.equal(response.output[0].content[0].text, '你好')
  assert.equal(emitted.filter((event) => event.type === 'response.completed').length, 1)
  assert.deepEqual(
    emitted.map((event) => event.sequence_number),
    emitted.map((_, index) => index)
  )
})

test('请求会复用账号浏览器版本及服务端更新 Cookie', () => {
  const ctx = context('chatgpt-api', ['auth.js', 'network_profile.js'])
  const auth = {
    auth: { fingerprint: { user_agent: 'Chrome/146.0.0.0' } },
    cookies: [{ name: 'a', value: 'old' }],
  }
  assert.equal(ctx.chatgptApiTlsProfile(auth), 'chrome_146')
  ctx.chatgptApiRememberCookies(auth, {
    headers: { 'set-cookie': 'a=new; Path=/\nb=2; Max-Age=300; Path=/' },
  })
  assert.equal(auth.cookies[0].value, 'new')
  assert.equal(auth.cookies[1].name, 'b')
})

test('Cookie 更新保持路径隔离，删除后不再发送', () => {
  const ctx = context('chatgpt-api', ['auth.js', 'network_profile.js'])
  const auth = {
    auth: {},
    cookies: [{ name: 'session', value: 'root', domain: '.chatgpt.com', path: '/', expires: -1 }],
  }
  ctx.chatgptApiRememberCookies(auth, {
    headers: { 'set-cookie': 'session=api; Path=/backend-api; Secure' },
  })
  assert.equal(auth.cookies.length, 2)
  assert.equal(ctx.chatgptApiCookieHeader(auth, '/api/auth/session'), 'session=root')
  assert.equal(ctx.chatgptApiCookieHeader(auth, '/backend-api/models'), 'session=api; session=root')
  ctx.chatgptApiRememberCookies(auth, {
    headers: { 'set-cookie': 'session=; Path=/backend-api; Max-Age=0' },
  })
  assert.equal(auth.cookies.length, 1)
  ctx.chatgptApiRememberCookies(auth, {
    headers: { 'set-cookie': 'session=external; Domain=example.org; Path=/' },
  })
  assert.equal(auth.cookies[0].value, 'root')
})

test('不支持的浏览器版本同时调整 UA、Client Hints 和 TLS', () => {
  const ctx = context('chatgpt-api', ['auth.js', 'network_profile.js'])
  const auth = {
    auth: {
      fingerprint: { user_agent: 'Mozilla/5.0 Chrome/150.0.0.0', device_id: 'stable-device' },
    },
  }
  ctx.chatgptApiAlignFingerprint(auth)
  assert.equal(ctx.chatgptApiTlsProfile(auth), 'chrome_149')
  assert.match(auth.auth.fingerprint.user_agent, /Chrome\/149\./)
  assert.equal(auth.auth.fingerprint.ua_full_version, '149.0.0.0')
  assert.equal(auth.auth.fingerprint.device_id, 'stable-device')
})

test('指定不存在的 Codex 工具必须失败，不能静默变更选择', () => {
  const ctx = context('codex', ['text_responses.js'])
  assert.throws(
    () =>
      ctx.codexNormalizeFunctionChoice({
        tools: [{ type: 'function', name: 'one' }],
        tool_choice: { type: 'function', name: 'other' },
      }),
    /invalid_input/
  )
})

test('Codex 模型发现过滤传输层不支持的思考深度', () => {
  const ctx = context('codex', ['list_models.js'])
  assert.equal(ctx.codexSupportedEffort('ultra'), false)
  assert.equal(ctx.codexSupportedEffort('high'), true)
})

test('文本图片输入保留图片并要求真实上传，不能静默丢弃', () => {
  const ctx = context('chatgpt-api', ['text_responses.js'])
  const input = ctx.chatgptApiNormalizeResponsesInput([
    {
      role: 'user',
      content: [{ type: 'input_image', image_url: 'https://example.test/image.png' }],
    },
  ])
  assert.equal(input.length, 1)
  assert.equal(input[0].images[0], 'https://example.test/image.png')
  assert.throws(() => ctx.chatgptApiUploadTextImages({}, { input }), /data URL/)
})

test('Codex 实时通道转换路由字段时保留原始请求', () => {
  const ctx = context('codex', ['realtime.js'])
  const request = { sdp: 'v=0', session: { model: 'gpt-live-1', instructions: 'hello' } }
  const upstream = ctx.codexRealtimeRequest(request)
  assert.equal(upstream.session.model, undefined)
  assert.equal(upstream.session.type, 'quicksilver')
  assert.equal(upstream.session.instructions, 'hello')
  assert.equal(request.session.model, 'gpt-live-1')
})

test('模型目录只公布网页文本通道可执行的模型', () => {
  const ctx = context('chatgpt-api', ['conversation.js', 'list_models.js'])
  ctx.chatgptApiRefreshSession = (value) => value
  ctx.chatgptApiReadAuth = () => ({})
  ctx.chatgptApiRequest = () => ({
    body: {
      models: [{ slug: 'gpt-5-6-instant' }, { slug: 'gpt-6-astra-wm' }, { slug: 'research' }],
    },
  })
  ctx.chatgptApiRequireOk = (value) => value
  const models = ctx.main({}).models
  assert.deepEqual(
    Array.from(models, (item) => item.model_id),
    ['gpt-5-6-instant']
  )
})

test('图片用量只汇总完整的图片 token，缺失时不能编造', () => {
  const ctx = context('codex', ['generate_image.js'])
  const item = {
    usage: {
      input_tokens: 3,
      output_tokens: 4,
      total_tokens: 7,
      input_tokens_details: { image_tokens: 1, text_tokens: 2 },
    },
  }
  assert.equal(ctx.codexImageUsage([item, item]).total_tokens, 14)
  assert.equal(ctx.codexImageUsage([{}]), undefined)
  assert.equal(
    ctx.codexImageUsage([
      {
        usage: {
          input_tokens: 3,
          output_tokens: 4,
          total_tokens: 7,
          input_tokens_details: { cached_tokens: 0 },
        },
      },
    ]),
    undefined
  )
})

test('模型家族目录包含可路由的关闭思考与思考档位', () => {
  const ctx = context('chatgpt-api', ['conversation.js'])
  const models = [
    { slug: 'gpt-5-6', thinking_efforts: [] },
    { slug: 'gpt-5-6-instant' },
    {
      slug: 'gpt-5-6-thinking',
      thinking_efforts: ['standard', 'extended'],
      default_thinking_effort: 'standard',
    },
  ]
  assert.deepEqual(Array.from(ctx.chatgptApiDiscoveredReasoning(models[0], models).efforts), [
    'none',
    'standard',
    'extended',
  ])
  assert.equal(ctx.chatgptApiModelSelection('gpt-5-6', 'none', models, {}).model, 'gpt-5-6-instant')
})

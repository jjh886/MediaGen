import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { resolve, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { test } from 'node:test'
const plugins = resolve(dirname(fileURLToPath(import.meta.url)), '../plugins')
const plain = (value) => JSON.parse(JSON.stringify(value))
function runtime(platform, saved = {}) {
  const host = {
    checkpoint(value) {
      if (value !== undefined) saved = plain(value)
      return plain(saved)
    },
    emit() {},
    call() {
      return {}
    },
    tryCall() {
      return { success: true }
    },
    sleep() {},
  }
  const ctx = vm.createContext({ host })
  const manifest = JSON.parse(readFileSync(join(plugins, platform, 'manifest.json'), 'utf8'))
  for (const file of manifest.flows.generate_text.steps[0].script_files) {
    const path = file.startsWith('@shared/')
      ? join(plugins, '_shared', file.slice(8))
      : join(plugins, platform, 'scripts', file)
    vm.runInContext(readFileSync(path, 'utf8'), ctx)
  }
  return { ctx, host }
}
const history = [
  { role: 'user', content: '我的暗号是青山' },
  { role: 'assistant', content: '记住了' },
  { role: 'user', content: '我的暗号是什么？' },
]

test('ChatGPT 两轮对话复用原编号及父消息，只发送新增输入，重启仍可继续', () => {
  const submitted = []
  const run = (input, session, saved = {}, returnedId) => {
    const { ctx } = runtime('chatgpt', saved)
    Object.assign(ctx, {
      chatgptApiReadAuth: () => ({}),
      chatgptApiRefreshSession: (a) => a,
      chatgptApiBootstrap: () => ({ '/backend-api/models': {} }),
      chatgptApiTextReasoning: () => {},
      chatgptApiTryOpenTextUpdates: () => null,
      chatgptApiInitializeConversation: () => ({}),
      chatgptApiTextOptions: () => ({ model: 'model' }),
      chatgptApiSentinelHeaders: () => ({}),
      chatgptApiPrepareTextConversation: (_a, _p, thread, options) => ({
        ...thread,
        ...options,
        user_message_id: 'user-' + (submitted.length + 1),
      }),
      chatgptApiStreamTextConversation: (auth, prompt, prepared, headers, attachments, event) => {
        submitted.push({ prompt, ...plain(prepared) })
        const id = returnedId || prepared.conversation_id || 'upstream-' + submitted.length
        event({
          conversation_id: id,
          message: {
            id: 'assistant-' + submitted.length,
            author: { role: 'assistant' },
            channel: 'final',
            content: { content_type: 'text', parts: ['青山'] },
            status: 'finished_successfully',
          },
        })
        return { ok: true, status: 200 }
      },
      chatgptApiFetchTextConversation: () => ({ title: '暗号' }),
    })
    return plain(
      ctx.main({
        request: {
          model: 'model',
          prompt_cache_key: 'window',
          input,
          options: { platform_session: session },
        },
      })
    ).response
  }
  const first = run([history[0]])
  const cursor = first.metadata.platform_session
  assert.deepEqual(cursor, { conversation_id: 'upstream-1', parent_message_id: 'assistant-1' })
  const second = run(history, JSON.parse(JSON.stringify(cursor)))
  assert.equal(submitted[1].conversation_id, 'upstream-1')
  assert.equal(submitted[1].parent_message_id, 'assistant-1')
  assert.ok(submitted[1].prompt.includes('我的暗号是什么'))
  assert.ok(!submitted[1].prompt.includes('我的暗号是青山'))
  assert.equal(second.metadata.platform_session.parent_message_id, 'assistant-2')
  run(history, cursor, {
    phase: 'retry_pending',
    reset_session: true,
    retry_not_before_ms: 0,
    failed_conversation_id: 'upstream-1',
  })
  assert.equal(submitted[2].conversation_id, 'upstream-1')
  assert.equal(submitted[2].parent_message_id, 'assistant-1')
  assert.ok(!submitted[2].prompt.includes('我的暗号是青山'))
  run([history[0]], null)
  assert.equal(submitted[3].conversation_id, '')
  assert.throws(() => run(history, cursor, {}, 'unexpected-session'), /绑定不一致/)
})

test('DeepSeek 后续请求使用原会话和上条回复编号，完成后推进游标', () => {
  const { ctx, host } = runtime('deepseek-api')
  const sent = []
  Object.assign(ctx, {
    apiSessionRead: () => ({}),
    deepseekInitializeSession: () => {},
    deepseekValidateAttachments: () => {},
    deepseekUploadAttachments: () => [],
    deepseekPow: () => '',
    deepseekHeaders: () => ({}),
    deepseekReportSettings: () => {},
    deepseekSessionTick: () => {},
    deepseekRefreshSession: () => {},
    deepseekSetting: (_a, _k, v) => v,
    deepseekRequest: (_a, path) => {
      assert.ok(path.startsWith('/chat/history_messages'))
      return { chat_session: { id: 'session-original', title: '原会话' } }
    },
  })
  host.stream = (_action, params, event) => {
    sent.push(plain(params.body))
    event({ event: 'ready', data: { response_message_id: 12 } })
    event({
      data: {
        v: {
          response: {
            message_id: 12,
            status: 'FINISHED',
            fragments: [{ type: 'RESPONSE', content: '青山' }],
          },
        },
      },
    })
    return { ok: true, status: 200 }
  }
  const result = plain(
    ctx.main({
      request: {
        model: 'deepseek',
        input: history,
        prompt_cache_key: 'window',
        options: {
          platform_session: { conversation_id: 'session-original', parent_message_id: 8 },
        },
      },
    })
  )
  assert.equal(sent[0].chat_session_id, 'session-original')
  assert.equal(sent[0].parent_message_id, 8)
  assert.ok(!sent[0].prompt.includes('我的暗号是青山'))
  assert.deepEqual(result.response.metadata.platform_session, {
    conversation_id: 'session-original',
    parent_message_id: 12,
  })
})

test('豆包继续原会话时沿用服务端最新 section/index，不创建新会话', () => {
  const { ctx, host } = runtime('doubao-api')
  const sent = []
  Object.assign(ctx, {
    apiSessionRead: () => ({}),
    doubaoRequireVerified: () => {},
    doubaoStartHeartbeats: () => {},
    doubaoUploadTextAttachments: () => [],
    doubaoMarkConversationRead: () => {},
    doubaoMessageId: () => 'local-message',
    doubaoLocalConversationId: () => 'local-conversation',
    doubaoImRequest: () => ({
      downlink_body: {
        get_conv_info_downlink_body: {
          conversation_info: {
            conversation_id: '123',
            last_section_id: '45',
            latest_index: 6,
            name: '原会话',
          },
        },
      },
    }),
    doubaoSubmit: (_session, payload, onText) => {
      sent.push(plain(payload))
      onText('青山')
      return { conversationId: '123', text: '青山', blocks: {} }
    },
  })
  const result = plain(
    ctx.main({
      request: {
        model: 'doubao',
        input: history,
        prompt_cache_key: 'window',
        options: { platform_session: { conversation_id: '123' } },
      },
    })
  )
  assert.equal(sent[0].client_meta.conversation_id, '123')
  assert.equal(sent[0].client_meta.last_section_id, '45')
  assert.equal(sent[0].client_meta.last_message_index, 6)
  assert.equal(sent[0].client_meta.local_conversation_id, undefined)
  assert.equal(sent[0].option.need_create_conversation, false)
  assert.ok(!JSON.stringify(sent[0].messages).includes('我的暗号是青山'))
  assert.equal(result.response.metadata.platform_session.conversation_id, '123')
})

test('Codex 同窗口使用稳定 session-id，同时完整携带历史上下文', () => {
  const { ctx } = runtime('codex')
  const first = ctx.codexUpstreamHeaders('token', 'account', 'text/event-stream', {
    request: { prompt_cache_key: 'window' },
    request_id: 'first',
  })
  const second = ctx.codexUpstreamHeaders('token', 'account', 'text/event-stream', {
    request: { prompt_cache_key: 'window' },
    request_id: 'second',
  })
  assert.equal(first['session-id'], 'window')
  assert.equal(second['session-id'], 'window')
  const request = ctx.codexNormalizeResponsesRequest(
    { input: history, model: 'gpt-6-luna', prompt_cache_key: 'window' },
    {}
  ).payload
  assert.equal(request.input.length, 3)
  assert.equal(request.prompt_cache_key, 'window')
})

test('续接同一平台时也保留期间由其他模型补入的上下文', () => {
  const { ctx } = runtime('chatgpt')
  const request = ctx.apiTextNormalizeTextRequest({
    input: [
      { role: 'user', content: '第一轮' },
      { role: 'assistant', content: '原平台已保存的回复' },
      { role: 'user', content: '中途的新信息' },
      { role: 'assistant', content: '另一模型补充的结论' },
      { role: 'user', content: '现在继续' },
    ],
    options: { platform_session: { conversation_id: 'original' }, session_input_start: 2 },
  })
  assert.deepEqual(
    plain(ctx.apiTextSessionRequest(request).input).map((item) => item.text),
    ['中途的新信息', '另一模型补充的结论', '现在继续']
  )
  request.session_input_start = 0
  assert.equal(ctx.apiTextSessionRequest(request).input.length, 5)
})

test('沿用平台历史时仍发送本轮更新的页面上下文和用户指令', () => {
  const { ctx } = runtime('chatgpt')
  const request = ctx.apiTextNormalizeTextRequest({
    instructions: '本轮用中文',
    input: [
      { role: 'system', content: '当前页面已经切换为 B' },
      { role: 'user', content: '旧问题' },
      { role: 'assistant', content: '旧回复' },
      { role: 'user', content: '这个页面是什么' },
    ],
    options: { platform_session: { conversation_id: 'original' }, session_input_start: 3 },
  })
  const continued = plain(ctx.apiTextSessionRequest(request))
  assert.equal(continued.instructions, '本轮用中文')
  assert.deepEqual(
    continued.input.map((item) => item.text),
    ['当前页面已经切换为 B', '这个页面是什么']
  )
})

test('豆包 SDK 从实际模块注册识别入口，支持改名和多分块依赖，拒绝缺失依赖', () => {
  for (const entry of ['ISb', 'Pf']) {
    const ctx = vm.createContext({})
    vm.runInContext(
      readFileSync(join(plugins, 'doubao-api/scripts/signing_environment.js'), 'utf8'),
      ctx
    )
    vm.runInContext(
      `
      globalThis.window = globalThis;
      globalThis.__LOADABLE_LOADED_CHUNKS__ = [
        [['dependency'], {YC2: function(module) { module.exports = { version: 'actual' } }}],
        [['sdk'], {${entry}: function(module, exports, require) {
          const dependency = require('YC2');
          window.bdms = { init() {}, version: dependency.version };
        }}]
      ];
      doubaoInitializeSecurityModule(globalThis);
    `,
      ctx
    )
    assert.equal(ctx.bdms.version, 'actual')
  }
  const missing = vm.createContext({})
  vm.runInContext(
    readFileSync(join(plugins, 'doubao-api/scripts/signing_environment.js'), 'utf8'),
    missing
  )
  assert.throws(
    () =>
      vm.runInContext(
        `
    globalThis.window=globalThis;
    globalThis.__LOADABLE_LOADED_CHUNKS__ = [[[], {Pf: function(m,e,r) {
      r('missing'); window.bdms={init(){}};
    }}]];
    doubaoInitializeSecurityModule(globalThis);
  `,
        missing
      ),
    /依赖缺失/
  )
  assert.throws(() => missing.doubaoInitializeSecurityModule({}), /无法唯一确定/)
})

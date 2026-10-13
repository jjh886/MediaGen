import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { test } from 'node:test'

const plugins = resolve(dirname(fileURLToPath(import.meta.url)), '../plugins')
const clone = (value) => JSON.parse(JSON.stringify(value))
function fixture(files, initial = {}) {
  const root = mkdtempSync(join(tmpdir(), 'plugin-restart-'))
  const file = join(root, 'checkpoint.json')
  writeFileSync(file, JSON.stringify(initial))
  const host = {
    checkpoint(value) {
      if (value !== undefined) writeFileSync(file, JSON.stringify(value))
      return JSON.parse(readFileSync(file, 'utf8'))
    },
    call: () => ({}),
    emit: () => {},
    sleep: () => {},
  }
  return {
    host,
    restart(overrides = {}) {
      const context = vm.createContext({ host, ...overrides })
      for (const name of files) vm.runInContext(readFileSync(join(plugins, name), 'utf8'), context)
      Object.assign(context, overrides)
      return (ctx) => clone(context.main(ctx))
    },
    close: () => rmSync(root, { recursive: true, force: true }),
  }
}
const shared = [
  '_shared/api_text_responses.js',
  '_shared/api_text_attachments.js',
  '_shared/api_text_media.js',
]

for (const interrupted of [false, true]) {
  test(`Codex 文本重启：${interrupted ? '无查询能力重试' : '已完成直接恢复含工具调用的结果'}`, () => {
    const f = fixture(['codex/scripts/generate_text.js'])
    let submissions = 0
    const response = {
      id: 'resp-original',
      output: [{ type: 'function_call', call_id: 'call-1', name: 'read', arguments: '{}' }],
    }
    const overrides = {
      codexNormalizeResponsesRequest: () => ({ payload: {} }),
      codexEnsureFreshAuth: () => ({}),
      codexApplyReasoning: () => {},
      codexExecuteResponses: () => {
        submissions++
        if (interrupted && submissions === 1) throw Error('测试中断')
        return { status: 200, response }
      },
    }
    try {
      if (interrupted) assert.throws(() => f.restart(overrides)({}), /测试中断/)
      else assert.deepEqual(f.restart(overrides)({}).response, response)
      assert.deepEqual(f.restart(overrides)({}).response, response)
      assert.deepEqual(f.restart({})({}).response, response)
      assert.equal(submissions, interrupted ? 2 : 1)
    } finally {
      f.close()
    }
  })
}

for (const mode of [
  'finished',
  'running',
  'missing-message-id',
  'linked-conversation',
  'network',
  'stopped',
  'no-record',
]) {
  test(`DeepSeek 重启：${mode}`, () => {
    const missing = ['missing-message-id', 'linked-conversation', 'no-record'].includes(mode)
    const f = fixture(
      [...shared, 'deepseek-api/scripts/stream.js', 'deepseek-api/scripts/generate_text.js'],
      {
        phase: missing && mode !== 'linked-conversation' ? 'submitting' : 'accepted',
        conversation_id: 'original',
        ...(missing ? {} : { response_message_id: 9 }),
      }
    )
    let queries = 0,
      submissions = 0,
      resumes = 0
    const overrides = {
      DEEPSEEK_ORIGIN: 'https://deepseek.test',
      apiSessionRead: () => ({}),
      deepseekValidateAttachments: () => {},
      deepseekInitializeSession: () => {},
      deepseekSetting: (_s, _k, v) => v,
      deepseekHeaders: () => ({}),
      deepseekReportSettings: () => {},
      deepseekSessionTick: () => {},
      deepseekRefreshSession: () => {},
      deepseekPow: () => '',
      deepseekUploadAttachments: () => [],
      apiSessionEventData: (e) => e,
      apiSessionRequireOk: (v) => v,
      deepseekRequest: (_s, path) => {
        if (path === '/chat_session/create') return { chat_session: { id: 'replacement' } }
        queries++
        if (mode === 'network') throw Error('temporary_unavailable: network')
        return {
          chat_session: { id: 'original', title: '原任务' },
          chat_messages:
            mode === 'no-record'
              ? []
              : [
                  {
                    message_id: 9,
                    role: 'ASSISTANT',
                    status:
                      mode === 'running' ? 'RUNNING' : mode === 'stopped' ? 'STOPPED' : 'FINISHED',
                    fragments: [
                      { type: 'RESPONSE', content: mode === 'running' ? '半' : '原结果' },
                    ],
                  },
                ],
        }
      },
    }
    f.host.stream = (_action, params, event) => {
      if (params.url.endsWith('/completion')) submissions++
      else resumes++
      event({ event: 'ready', response_message_id: 10 })
      event({
        v: {
          response: {
            message_id: 10,
            status: 'FINISHED',
            fragments: [{ type: 'RESPONSE', content: mode === 'running' ? '半完成' : '新结果' }],
          },
        },
      })
      return { ok: true }
    }
    const ctx = { request: { input: '原请求', model: 'deepseek' } }
    try {
      if (mode === 'network') {
        assert.throws(() => f.restart(overrides)(ctx), /network/)
        assert.equal(f.host.checkpoint().conversation_id, 'original')
      } else {
        const result = f.restart(overrides)(ctx)
        assert.equal(
          result.response.output_text,
          mode === 'running'
            ? '半完成'
            : ['stopped', 'no-record'].includes(mode)
              ? '新结果'
              : '原结果'
        )
        const count = queries
        assert.deepEqual(f.restart({})(ctx), result)
        assert.equal(queries, count)
      }
      assert.equal(submissions, ['stopped', 'no-record'].includes(mode) ? 1 : 0)
      assert.equal(resumes, mode === 'running' ? 1 : 0)
    } finally {
      f.close()
    }
  })
}

for (const kind of ['text', 'video', 'image']) {
  test(`豆包 ${kind}：受理后销毁运行环境，重启只查询同一会话`, () => {
    const f = fixture([...shared, `doubao-api/scripts/generate_${kind}.js`])
    let submissions = 0
    const queried = []
    const creations = [
      {
        image: {
          key: 'img',
          image_ori_raw: { url: 'https://media.test/image', width: 1024, height: 1024 },
        },
        video: { vid: 'v', duration: 6 },
      },
    ]
    const overrides = {
      apiSessionRead: () => ({}),
      doubaoRequireVerified: () => {},
      doubaoStartHeartbeats: () => {},
      doubaoMediaOptions: () => ({ model: 'model', ratio: 'auto' }),
      doubaoUploadReferences: () => [],
      doubaoUploadTextAttachments: () => [],
      doubaoReferenceBlocks: () => [],
      doubaoPayload: () => ({}),
      doubaoTextMediaState: () => ({ completed: {} }),
      doubaoMarkConversationRead: () => {},
      doubaoReadTextTitle: () => '',
      doubaoCreations: () => [],
      doubaoVideoDownload: () => 'https://media.test/video',
      doubaoSubmit: () => {
        submissions++
        f.host.checkpoint({ phase: 'accepted', conversation_id: '123' })
        throw Error('测试中断')
      },
      doubaoWaitForResult: (_s, id) => {
        queried.push(id)
        return creations
      },
      doubaoWaitForTextMedia: (_s, _m, writer, id) => {
        queried.push(id)
        writer.parser.buffer = '原结果'
      },
    }
    const ctx = { prompt: '原请求', max_images: 1, request: { input: '原请求', model: 'model' } }
    try {
      assert.throws(() => f.restart(overrides)(ctx), /测试中断/)
      const result = f.restart(overrides)(ctx)
      assert.equal(result.success, true)
      assert.equal(submissions, 1)
      assert.deepEqual(queried, ['123'])
      assert.equal(f.host.checkpoint().phase, 'completed')
    } finally {
      f.close()
    }
  })
}

test('ChatGPT 图片：重启后先查原会话，不发送已过期的流恢复令牌', () => {
  const f = fixture(['chatgpt/scripts/conversation.js'], {
    phase: 'accepted',
    conversation_id: 'original',
    resume_token: 'expired',
  })
  let queries = 0
  try {
    const context = vm.createContext({
      host: f.host,
      chatgptApiWaitForImage: (_auth, id) => {
        assert.equal(id, 'original')
        queries++
        return { url: 'data:image/png;base64,AAA' }
      },
    })
    vm.runInContext(readFileSync(join(plugins, 'chatgpt/scripts/conversation.js'), 'utf8'), context)
    assert.equal(
      context.chatgptApiGenerateConversation({}, 'prompt', 'auto', [], {}).result.url,
      'data:image/png;base64,AAA'
    )
    assert.equal(queries, 1)
  } finally {
    f.close()
  }
})

for (const kind of ['text', 'image', 'audio', 'transcribe']) {
  test(`Grok ${kind}：无查询接口时重试，完成结果跨运行环境保存`, () => {
    const name = kind === 'transcribe' ? 'transcribe_audio' : `generate_${kind}`
    const f = fixture([`grok/scripts/${name}.js`], { phase: 'submitting' })
    let submissions = 0
    const overrides = {
      grokAuth: () => ({}),
      grokHeaders: () => ({}),
      grokText: () => {
        submissions++
        return { id: 'response', output_text: '原结果' }
      },
      grokRequest: () => {
        submissions++
        return { body: { text: '转写', data: [{ b64_json: 'aGVsbG8=' }] }, body_base64: 'aGVsbG8=' }
      },
    }
    const ctx = {
      model_id: 'model',
      prompt: '提示词',
      request: { model: 'model', input: '提示词', file: 'audio' },
    }
    if (kind === 'transcribe') delete ctx.request.input
    try {
      const result = f.restart(overrides)(ctx)
      assert.equal(submissions, 1)
      assert.deepEqual(f.restart({})(ctx), result)
      assert.equal(submissions, 1)
    } finally {
      f.close()
    }
  })
}

test('Grok 视频：上游受理后中断，重启查询原任务，不再创建视频', () => {
  const f = fixture(['grok/scripts/generate_video.js'])
  let submissions = 0,
    queries = 0
  const overrides = {
    grokAuth: () => ({}),
    grokRequest: (_ctx, _auth, path, body) => {
      if (body) {
        submissions++
        return { body: { request_id: 'original-job' } }
      }
      queries++
      assert.equal(path, '/videos/original-job')
      if (queries === 1) throw Error('测试中断')
      return { body: { video_url: 'https://api.x.ai/video.mp4' } }
    },
  }
  try {
    assert.throws(() => f.restart(overrides)({ prompt: '提示词' }), /测试中断/)
    assert.equal(
      f.restart(overrides)({ prompt: '提示词' }).videos[0].video_url,
      'https://api.x.ai/video.mp4'
    )
    assert.equal(submissions, 1)
    assert.equal(queries, 2)
  } finally {
    f.close()
  }
})

test('ChatGPT 会话暂未可见先重查，连续不存在才重新提交，网络/限流保留原编号', () => {
  const f = fixture(['chatgpt/scripts/poll_retry.js'], {
    phase: 'accepted',
    conversation_id: 'original',
    images: [{ url: 'data:image/png;base64,saved' }],
  })
  const context = vm.createContext({ host: f.host })
  vm.runInContext(readFileSync(join(plugins, 'chatgpt/scripts/poll_retry.js'), 'utf8'), context)
  try {
    for (const status of [503, 429, 401]) {
      assert.equal(context.chatgptApiMissingConversation({ status }), false)
      assert.equal(f.host.checkpoint().conversation_id, 'original')
    }
    assert.equal(context.chatgptApiMissingConversation({ status: 404 }), true)
    assert.equal(context.chatgptApiMissingConversation({ status: 404 }), true)
    assert.throws(() => context.chatgptApiMissingConversation({ status: 404 }), /重新提交/)
    assert.equal(f.host.checkpoint().phase, 'ready')
    assert.equal(f.host.checkpoint().conversation_id, undefined)
    assert.equal(f.host.checkpoint().images.length, 1)
  } finally {
    f.close()
  }
})

for (const failure of [
  'WebSocket 读取失败',
  'WebSocket 心跳失败',
  'WebSocket 连接已关闭',
  'closed',
]) {
  for (const rateLimited of [false, true]) {
    test(`ChatGPT ${failure}：原会话轮询完成，保留限流等待=${rateLimited}`, () => {
      const manifest = JSON.parse(readFileSync(join(plugins, 'chatgpt/manifest.json'), 'utf8'))
      const files = manifest.flows.generate_text.steps[0].script_files.map((name) =>
        name.startsWith('@shared/') ? '_shared/' + name.slice(8) : 'chatgpt/scripts/' + name
      )
      const f = fixture(files, {
        phase: 'accepted',
        conversation_id: 'original',
        user_message_id: 'user',
        poll_rate_limited: rateLimited,
        poll_next_at_ms: rateLimited ? 90000 : 0,
      })
      let clock = 0,
        reads = 0,
        closes = 0
      const polls = []
      const context = vm.createContext({
        host: f.host,
        Date: class extends Date {
          static now() {
            return clock
          }
        },
      })
      for (const file of files) vm.runInContext(readFileSync(join(plugins, file), 'utf8'), context)
      f.host.tryCall = (action) => {
        assert.equal(action, 'http.websocket_close')
        closes++
        return { success: true }
      }
      f.host.call = (action, params) => {
        if (action === 'time.sleep') {
          clock += params.ms
          return {}
        }
        assert.equal(action, 'http.websocket_receive', '轮询恢复不能重新提交请求')
        reads++
        if (failure === 'closed') return { closed: true }
        throw Error('平台错误: ' + failure)
      }
      context.chatgptApiOpenTextUpdates = () => {
        throw Error('断线后不应反复创建通知连接')
      }
      context.chatgptApiTextConversationResponse = (_auth, id) => {
        assert.equal(id, 'original')
        polls.push(clock)
        const user = {
          id: 'user',
          author: { role: 'user' },
          content: { content_type: 'text', parts: ['问题'] },
        }
        const answer = {
          id: 'answer',
          author: { role: 'assistant' },
          content: { content_type: 'text', parts: ['原会话完成'] },
          status: 'finished_successfully',
          channel: 'final',
        }
        return {
          ok: true,
          status: 200,
          body: { messages: polls.length === 1 ? [user] : [user, answer] },
        }
      }
      try {
        const result = vm.runInContext(
          `chatgptApiWaitForRichText({},'original','root',chatgptApiCreateTextStreamState(),apiTextCreateResponseWriter(apiTextNormalizeTextRequest({input:'问题',model:'gpt-6-pro'})),{connection_id:'ws'})`,
          context
        )
        assert.equal(result, '原会话完成')
        assert.deepEqual(polls, rateLimited ? [90000, 120000] : [0, 30000])
        assert.equal(reads, 1)
        assert.equal(closes, 1)
        assert.equal(f.host.checkpoint().conversation_id, 'original')
      } finally {
        f.close()
      }
    })
  }
}

test('ChatGPT 通知接收不吞掉权限错误或消息处理错误，空闲超时不误判断线', () => {
  const f = fixture(['chatgpt/scripts/text_updates.js'])
  const context = vm.createContext({ host: f.host })
  vm.runInContext(readFileSync(join(plugins, 'chatgpt/scripts/text_updates.js'), 'utf8'), context)
  const receive = (consume = () => {}) =>
    context.chatgptApiReceiveTextUpdates({ connection_id: 'ws' }, 'original', consume)
  try {
    f.host.call = () => {
      throw Error('capability denied: http.websocket_receive')
    }
    assert.throws(() => receive(), /capability denied/)
    f.host.call = () => ({ timeout: true })
    assert.equal(receive(), undefined)
    f.host.call = () => ({
      text: JSON.stringify({
        type: 'conversation-update',
        payload: { conversation_id: 'original', update_content: { messages: [{}] } },
      }),
    })
    assert.throws(
      () =>
        receive(() => {
          throw Error('WebSocket 读取失败：来自消息处理回调')
        }),
      /来自消息处理回调/
    )
  } finally {
    f.close()
  }
})

import console from 'node:console'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import { beforeEach, describe, expect, it } from 'vitest'

const scriptDirectory = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../../plugins/chatgpt/scripts'
)

let context
let emitted

beforeEach(() => {
  emitted = []
  context = vm.createContext({
    console,
    host: {
      checkpoint: () => ({}),
      call: () => ({}),
      stream: () => ({}),
      emit: (event) => emitted.push(JSON.parse(JSON.stringify(event))),
    },
  })
  for (const name of [
    '../../_shared/api_session.js',
    'auth.js',
    'network_profile.js',
    'startup.js',
    'sentinel_vm.js',
    'sentinel.js',
    'conversation.js',
    'conversation_results.js',
    '../../_shared/api_text_attachments.js',
    '../../_shared/api_text_responses.js',
    'reference_upload.js',
    '../../_shared/api_text_media.js',
    'text_updates.js',
    'text_media.js',
    'poll_retry.js',
    'text_conversation.js',
    'generate_text.js',
    'audio_speech.js',
    'generate_audio.js',
    'check_account.js',
  ]) {
    vm.runInContext(readFileSync(path.join(scriptDirectory, name), 'utf8'), context, {
      filename: name,
    })
  }
})

function evaluate(source, values = {}) {
  Object.assign(context, values)
  return JSON.parse(vm.runInContext(`JSON.stringify(${source})`, context))
}

describe('ChatGPT API 文本协议', () => {
  it('网络检查失败保留账号状态，明确登录过期才标记过期', () => {
    expect(evaluate(`chatgptApiAccountCheckError(new Error('HTTP 错误: connect'))`)).toMatchObject({
      logged_in: null,
      status: 'inconclusive',
    })
    expect(
      evaluate(`chatgptApiAccountCheckError(new Error('LOGIN_EXPIRED: 登录凭据无效'))`)
    ).toMatchObject({
      logged_in: false,
      status: 'login_expired',
    })
  })
  it('保留视频和音轨，并按 ChatGPT 附件协议发送', () => {
    const request = evaluate(`apiTextNormalizeTextRequest(__request)`, {
      __request: {
        model: 'gpt-6-pro',
        input: [
          {
            role: 'user',
            content: [
              { type: 'input_text', text: '描述画面和对白' },
              {
                type: 'input_file',
                filename: 'episode.mp4',
                file_data: 'data:video/mp4;base64,YWJj',
              },
              { type: 'input_audio', input_audio: { format: 'wav', data: 'YWJj' } },
            ],
          },
        ],
      },
    })
    expect(request.input[0].attachments).toMatchObject([
      { name: 'episode.mp4', mime_type: 'video/mp4', size: 3 },
      { mime_type: 'audio/wav', size: 3 },
    ])
    const attachments = [
      {
        file_id: 'file-video',
        library_file_id: 'lib-video',
        name: 'episode.mp4',
        mime_type: 'video/mp4',
        size: 3,
      },
      {
        file_id: 'file-audio',
        library_file_id: 'lib-audio',
        name: 'audio.wav',
        mime_type: 'audio/wav',
        size: 3,
      },
    ]
    const auth = { runtime_started_at: Date.now(), auth: { fingerprint: {} } }
    const prepare = evaluate(
      `chatgptApiTextPreparePayload(__auth, '描述画面和对白', { parent_message_id: 'root' }, { model: 'gpt-6-pro' }, 'none', __attachments)`,
      { __auth: auth, __attachments: attachments }
    )
    expect(prepare.attachment_mime_types).toEqual(['video/mp4', 'audio/wav'])
    const payload = evaluate(
      `chatgptApiTextConversationPayload(__auth, '描述画面和对白', { parent_message_id: 'root', model: 'gpt-6-pro', user_message_id: 'msg' }, __attachments)`,
      { __auth: auth, __attachments: attachments }
    )
    expect(payload.messages[0].metadata.attachments).toEqual(
      attachments.map((item) => ({
        id: item.file_id,
        library_file_id: item.library_file_id,
        mime_type: item.mime_type,
        name: item.name,
        size: item.size,
      }))
    )
  })

  it('用 ace_upload 完成视频上传，不调用图片专用校验', () => {
    vm.runInContext(
      `
      globalThis.__uploadRequests = []
      globalThis.__blobHeaders = null
      chatgptApiRequest = function (_auth, method, requestPath, options) {
        globalThis.__uploadRequests.push({ method, path: requestPath, body: options.body })
        if (requestPath === '/backend-api/files/upload_reservations') {
          return { ok: true, body: { eligible: true } }
        }
        if (requestPath === '/backend-api/files') {
          return { ok: true, body: { file_id: 'file-1', upload_url: 'https://blob.core.windows.net/upload' } }
        }
        if (requestPath === '/backend-api/files/process_upload_stream') {
          return { ok: true, body_text: JSON.stringify({ event: 'file.processing.completed' }) + '\\n' + JSON.stringify({ event: 'file.indexing.completed', extra: { metadata_object_id: 'lib-1', library_file_name: 'episode.mp4', mime_type: 'video/mp4' } }) }
        }
        throw new Error('意外的图片专用校验：' + requestPath)
      }
      host.call = function (action, params) {
        if (action === 'file.write_base64') return { path: params.path, size: 3 }
        if (action === 'http.request') {
          globalThis.__blobHeaders = params.headers
          return { ok: true }
        }
        throw new Error('意外的原子操作：' + action)
      }
      `,
      context
    )
    const uploaded = evaluate(`chatgptApiUploadTextAttachments(__ctx, __auth, __input)`, {
      __ctx: { workspace_dir: '/tmp/attachment-test' },
      __auth: { auth: { fingerprint: {} } },
      __input: [
        { attachments: [{ name: 'episode.mp4', mime_type: 'video/mp4', base64: 'YWJj', size: 3 }] },
      ],
    })
    expect(uploaded[0]).toMatchObject({
      file_id: 'file-1',
      library_file_id: 'lib-1',
      name: 'episode.mp4',
      mime_type: 'video/mp4',
      size: 3,
    })
    const requests = evaluate('globalThis.__uploadRequests')
    expect(requests.map((item) => item.path)).toEqual([
      '/backend-api/files/upload_reservations',
      '/backend-api/files',
      '/backend-api/files/process_upload_stream',
    ])
    expect(requests[1].body).toMatchObject({ use_case: 'ace_upload', mime_type: 'video/mp4' })
    expect(requests[2].body).toMatchObject({ use_case: 'ace_upload' })
    expect(evaluate('globalThis.__blobHeaders')).toEqual({
      'content-type': 'video/mp4',
      'x-ms-blob-type': 'BlockBlob',
    })
  })

  it('拒绝将 Work 专用模型通过普通会话静默路由为其他模型', () => {
    expect(() =>
      evaluate(
        `chatgptApiTextOptions({}, { model: 'gpt-6-astra-wm', reasoning: { effort: 'standard' } })`
      )
    ).toThrow('Work 模型')
  })
  it('只接受通用 request，并校验标准文本参数', () => {
    expect(
      evaluate(`apiTextNormalizeTextRequest(__request)`, {
        __request: {
          model: 'chatgpt-web',
          input: '  已由业务侧渲染的输入  ',
          reasoning: { effort: 'high' },
          max_output_tokens: 12000,
          stream: true,
        },
      })
    ).toEqual({
      model: 'chatgpt-web',
      input: [{ role: 'user', text: '已由业务侧渲染的输入' }],
      instructions: '',
      max_output_tokens: 12000,
      reasoning: { effort: 'high' },
      tools: [],
      tool_choice: 'none',
      parallel_tool_calls: true,
      text: null,
      stream: true,
    })
    expect(() => vm.runInContext(`apiTextNormalizeTextRequest({ input: '' })`, context)).toThrow(
      'missing_input'
    )
    expect(() =>
      vm.runInContext(
        `apiTextNormalizeTextRequest({ input: '文本', reasoning: { effort: 'invalid effort' } })`,
        context
      )
    ).toThrow('invalid_input')
  })

  it('按网页准备 sent、读取能力，并保持发送状态和流编码一致', () => {
    vm.runInContext(
      `
      globalThis.__requests = []
      chatgptApiRequest = function (_auth, method, requestPath, options) {
        globalThis.__requests.push({ method: method, path: requestPath, options: options })
        return { ok: true, body: { status: 'ok', conduit_token: 'token-' + globalThis.__requests.length } }
      }
      `,
      context
    )
    const auth = {
      runtime_started_at: Date.now(),
      auth: { fingerprint: { timezone: 'Asia/Shanghai', timezone_offset_min: -480 } },
    }
    const thread = {
      conversation_id: 'conversation-1',
      parent_message_id: 'parent-1',
      reused: true,
    }
    const options = { model: 'auto', thinking_effort: 'standard' }
    const prepared = evaluate(
      `chatgptApiPrepareTextConversation(__auth, '通用文本输入', __thread, __options, {})`,
      { __auth: auth, __thread: thread, __options: options }
    )
    const requests = evaluate('globalThis.__requests')

    expect(
      requests
        .filter((item) => item.method === 'POST')
        .map((item) => item.options.body.client_prepare_state)
    ).toEqual(['sent'])
    expect(requests[1].path).toBe('/backend-api/system_hints')
    expect(requests[0].options.body.partial_query.content.parts[0]).toBe('通用文本输入')

    const payload = evaluate(
      `chatgptApiTextConversationPayload(__auth, '通用文本输入', __prepared)`,
      { __auth: auth, __prepared: prepared }
    )
    expect(payload).toMatchObject({
      parent_message_id: 'parent-1',
      conversation_id: 'conversation-1',
      client_prepare_state: 'sent',
      is_do_not_remember: false,
      thinking_effort: 'standard',
      supported_encodings: ['v1'],
      turn_attribution: { turn_trigger: 'composer' },
      client_contextual_info: { app_name: 'chatgpt.com' },
    })
    expect(payload.messages[0].metadata).toEqual({})
    for (const key of [
      'enable_message_followups',
      'supports_buffering',
      'history_and_training_disabled',
    ]) {
      expect(payload).not.toHaveProperty(key)
    }
  })

  it('SSE 只返回 final channel 并能应用批量 patch', () => {
    const direct = evaluate(`chatgptApiParseTextEvents(__events)`, {
      __events: [
        {
          v: {
            conversation_id: 'conversation-thinking',
            message: {
              id: 'message-analysis',
              author: { role: 'assistant' },
              channel: 'analysis',
              content: { parts: ['内部分析'] },
            },
          },
        },
        {
          v: {
            conversation_id: 'conversation-thinking',
            message: {
              id: 'message-final',
              author: { role: 'assistant' },
              channel: 'final',
              content: { parts: ['最终文本'] },
              end_turn: true,
            },
          },
        },
      ],
    })
    expect(direct).toEqual({
      conversation_id: 'conversation-thinking',
      output_text: '最终文本',
    })

    const patched = evaluate(`chatgptApiParseTextEvents(__events)`, {
      __events: [
        { p: '/conversation_id', o: 'replace', v: 'conversation-patch' },
        { p: '/message/id', o: 'replace', v: 'message-patch' },
        { p: '/message/author/role', o: 'replace', v: 'assistant' },
        { p: '/message/channel', o: 'replace', v: 'final' },
        { p: '/message/content/parts/0', o: 'append', v: '文本一' },
        {
          p: '',
          o: 'patch',
          v: [
            { p: '/message/content/parts/0', o: 'append', v: '，文本二' },
            { p: '/message/end_turn', o: 'replace', v: true },
          ],
        },
      ],
    })
    expect(patched).toEqual({
      conversation_id: 'conversation-patch',
      output_text: '文本一，文本二',
    })

    const unfinished = evaluate(`chatgptApiParseTextEvents(__events)`, {
      __events: [
        { p: '/conversation_id', o: 'replace', v: 'conversation-unfinished' },
        { p: '/message/id', o: 'replace', v: 'message-unfinished' },
        { p: '/message/author/role', o: 'replace', v: 'assistant' },
        { p: '/message/channel', o: 'replace', v: 'final' },
        { p: '/message/content/parts/0', o: 'append', v: '尚未结束的正文' },
      ],
    })
    expect(unfinished).toEqual({
      conversation_id: 'conversation-unfinished',
      output_text: '',
    })
  })

  it('接收新版无路径补丁数组，并保留重复字词和工具标签', () => {
    const result = evaluate('chatgptApiParseTextEvents(__events)', {
      __events: [
        {
          v: {
            conversation_id: 'conv',
            message: {
              id: 'msg',
              author: { role: 'assistant' },
              channel: 'final',
              content: { parts: ['好'] },
            },
          },
        },
        { v: [{ p: '/message/content/parts/0', o: 'append', v: '好' }] },
        {
          o: 'patch',
          v: [
            {
              p: '/message/content/parts/0',
              o: 'append',
              v: '<tool_call>{"name":"read","arguments":{}}</tool_call>',
            },
            { p: '/message/end_turn', o: 'replace', v: true },
          ],
        },
      ],
    })
    expect(result.output_text).toBe('好好<tool_call>{"name":"read","arguments":{}}</tool_call>')
  })

  it.each(['gpt-5-6-t-mini', 'gpt-5-6-thinking'])(
    '%s 在准备和提交时使用所选型号，不替换为 auto',
    (model) => {
      const options = evaluate('chatgptApiTextOptions({}, __request)', {
        __request: { model, reasoning: { effort: 'extended' } },
      })
      expect(options).toEqual({
        model,
        system_hints: [],
        thinking_effort: 'extended',
      })
      expect(
        evaluate('chatgptApiTextPreparePayload({}, "测试", {}, __options, "none")', {
          __options: options,
        })
      ).toMatchObject({ model })
      expect(
        evaluate('chatgptApiTextConversationPayload({}, "测试", __options)', { __options: options })
      ).toMatchObject({ model })
    }
  )

  it('轮询会话树只采用已确认结束的 final 正文', () => {
    const inProgress = evaluate(
      `chatgptApiFinalTextFromConversation(__conversation, 'message-parent')`,
      {
        __conversation: {
          current_node: 'message-final',
          mapping: {
            'message-final': {
              parent: 'message-parent',
              message: {
                author: { role: 'assistant' },
                channel: 'final',
                status: 'in_progress',
                content: { parts: ['尚未生成完整文本'] },
              },
            },
            'message-parent': { parent: null },
          },
        },
      }
    )
    expect(inProgress).toBe('')

    const completed = evaluate(
      `chatgptApiFinalTextFromConversation(__conversation, 'message-parent')`,
      {
        __conversation: {
          current_node: 'message-final',
          mapping: {
            'message-final': {
              parent: 'message-parent',
              message: {
                author: { role: 'assistant' },
                channel: 'final',
                status: 'finished_successfully',
                content: { parts: ['完整文本正文'] },
              },
            },
            'message-parent': { parent: null },
          },
        },
      }
    )
    expect(completed).toBe('完整文本正文')
  })

  it('请求参数完全来自通用模型请求，不读取剧本或插件全局参数', () => {
    expect(
      evaluate(`chatgptApiTextOptions(__init, __request)`, {
        __init: { default_model_slug: 'account-default' },
        __request: {
          model: 'chatgpt-web',
          input: [{ role: 'user', text: '任意文本' }],
          reasoning: { effort: 'extended' },
        },
      })
    ).toEqual({ model: 'account-default', system_hints: [], thinking_effort: 'extended' })

    expect(
      evaluate(`chatgptApiTextOptions(__init, __request)`, {
        __init: { default_model_slug: 'account-default' },
        __request: {
          model: 'gpt-5-5',
          input: [{ role: 'user', text: '任意文本' }],
          reasoning: { effort: 'standard' },
        },
      })
    ).toEqual({ model: 'gpt-5-5', system_hints: [], thinking_effort: 'standard' })
  })

  it('把 ChatGPT 私有增量实时转换为完整 Responses 生命周期', () => {
    vm.runInContext(
      `
      chatgptApiOpenTextUpdates = function () { return {} }
      chatgptApiCloseTextUpdates = function () {}
      chatgptApiInitializeConversation = function () { return { default_model_slug: 'account-model' } }
      chatgptApiSentinelHeaders = function () { return {} }
      chatgptApiPrepareTextConversation = function (_auth, _prompt, thread, options) {
        return { ...thread, ...options, turn_trace_id: 'trace-1', conduit: '', user_message_id: 'user-1' }
      }
      chatgptApiHideConversation = function () {}
      host.stream = function (_action, _params, onEvent) {
        onEvent({ p: '/conversation_id', o: 'replace', v: 'conversation-1' })
        onEvent({ p: '/message/id', o: 'replace', v: 'message-1' })
        onEvent({ p: '/message/author/role', o: 'replace', v: 'assistant' })
        onEvent({ p: '/message/channel', o: 'replace', v: 'final' })
        onEvent({ p: '/message/content/parts/0', o: 'append', v: '你' })
        onEvent({ p: '/message/content/parts/0', o: 'append', v: '好' })
        onEvent({ p: '/message/end_turn', o: 'replace', v: true })
        return { ok: true, status: 200, events: [] }
      }
      `,
      context
    )
    const response = evaluate(
      `chatgptApiGenerateTextConversation(__auth, apiTextNormalizeTextRequest({ model: 'chatgpt-web', input: '问候', stream: true }))`,
      {
        __auth: {
          runtime_started_at: Date.now(),
          runtime_session_id: 'session-1',
          cookies: [],
          auth: { access_token: 'token', account: { id: 'account-1' }, fingerprint: {} },
        },
      }
    )
    expect(response).toMatchObject({
      object: 'response',
      status: 'completed',
      model: 'account-model',
      output_text: '你好',
      usage: null,
    })
    expect(emitted.map((event) => event.type)).toEqual([
      'response.created',
      'response.in_progress',
      'response.output_item.added',
      'response.content_part.added',
      'response.output_text.delta',
      'response.output_text.delta',
      'response.output_text.done',
      'response.content_part.done',
      'response.output_item.done',
      'response.completed',
    ])
  })

  it('工具标签转换为 function_call 且不会进入正文', () => {
    const request = evaluate(`apiTextNormalizeTextRequest(__request)`, {
      __request: {
        input: '读取文件',
        tools: [
          {
            type: 'function',
            name: 'read_file',
            description: '读取文件',
            parameters: { type: 'object' },
          },
        ],
        stream: true,
      },
    })
    const response = evaluate(
      `(() => {
        const writer = apiTextCreateResponseWriter(__request)
        apiTextStartResponse(writer)
        apiTextFeedResponseText(writer, '<tool_')
        apiTextFeedResponseText(writer, 'call>{"name":"read_file","arguments":{"path":"a.txt"}}</tool_call>')
        return apiTextCompleteResponse(writer)
      })()`,
      { __request: request }
    )
    expect(response.output_text).toBe('')
    expect(response.output[0]).toMatchObject({
      type: 'function_call',
      name: 'read_file',
      arguments: '{"path":"a.txt"}',
    })
    expect(emitted.some((event) => event.type === 'response.function_call_arguments.delta')).toBe(
      true
    )
  })

  it('请求头携带账号标识和完整 Sentinel token', () => {
    const auth = {
      runtime_started_at: Date.now(),
      runtime_session_id: 'session-1',
      cookies: [{ name: 'oai-did', value: 'device-1' }],
      auth: {
        access_token: 'access-token',
        account: { id: 'account-1' },
        fingerprint: { language: 'zh-CN', user_agent: 'Test Browser' },
      },
    }
    const headers = evaluate(
      `chatgptApiBaseHeaders(__auth, '/backend-api/f/conversation', '/backend-api/f/conversation')`,
      { __auth: auth }
    )
    expect(headers['chatgpt-account-id']).toBe('account-1')
    expect(headers['oai-device-id']).toBe('device-1')

    vm.runInContext(
      `
      chatgptApiBase64Encode = function (value) { return String(value) }
      chatgptApiCreateVm = function () {
        return {
          decodeProgram: function (value) { return value },
          executeInstructions: function (value) { return value === 'snapshot-dx' ? 'so-result' : '' },
        }
      }
      chatgptApiRequest = function (_auth, method, requestPath) {
        if (method === 'POST' && requestPath.endsWith('/prepare')) {
          return {
            ok: true,
            body: {
              token: 'chat-1',
              prepare_token: 'prepare-1',
              proofofwork: {},
              turnstile: {},
              so: { required: true, collector_dx: 'collector-dx', snapshot_dx: 'snapshot-dx' },
            },
          }
        }
        return { ok: true, body: { token: 'requirements-1' } }
      }
      `,
      context
    )
    const sentinel = evaluate(`chatgptApiSentinelHeaders(__auth)`, { __auth: auth })
    expect(sentinel['openai-sentinel-chat-requirements-token']).toBe('requirements-1')
    expect(sentinel['openai-sentinel-chat-requirements-prepare-token']).toBeUndefined()
    expect(sentinel['oai-telemetry']).toBeUndefined()
    expect(JSON.parse(sentinel['openai-sentinel-so-token'])).toEqual({
      so: 'so-result',
      c: 'chat-1',
      id: 'device-1',
      flow: 'device-1',
    })
  })
})

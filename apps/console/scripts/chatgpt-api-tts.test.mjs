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

beforeEach(() => {
  context = vm.createContext({ console, host: { call: () => ({}) } })
  for (const name of [
    '../../_shared/api_session.js',
    'auth.js',
    'network_profile.js',
    'conversation.js',
    '../../_shared/api_text_attachments.js',
    '../../_shared/api_text_responses.js',
    'conversation_results.js',
    '../../_shared/api_text_media.js',
    'text_updates.js',
    'text_media.js',
    'poll_retry.js',
    'text_conversation.js',
    'audio_speech.js',
    'generate_audio.js',
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

describe('ChatGPT API TTS 协议', () => {
  it('只朗读完整原文，后续空消息和聊天回复不能替换原文消息', () => {
    const message = (id, text) => ({
      v: {
        conversation_id: 'speech',
        message: {
          id,
          author: { role: 'assistant' },
          content: { parts: [text] },
        },
      },
    })
    expect(
      evaluate(`chatgptApiSpeechMessageIds(__events, '你好。')`, {
        __events: [
          message('input', '你好。'),
          message('reply', ''),
          message('reply', '你好。今天想聊什么？'),
        ],
      })
    ).toEqual({ conversation_id: 'speech', message_id: 'input' })
    expect(
      evaluate(`chatgptApiSpeechMessageIds(__events, '你好。')`, {
        __events: [message('reply', '你好。'), message('reply', '你好。今天想聊什么？')],
      })
    ).toEqual({ conversation_id: 'speech', message_id: '' })
    expect(
      evaluate(`chatgptApiSpeechMessageIds(__events, '你好。')`, {
        __events: [message('reply', '其他内容')],
      })
    ).toEqual({ conversation_id: 'speech', message_id: '' })
  })

  it('映射 OpenAI 音色及原生容器，转码由入口完成', () => {
    expect(evaluate(`chatgptApiSpeechVoice('nova')`)).toEqual({
      requested_voice: 'nova',
      platform_voice: 'maple',
    })
    expect(() => evaluate(`chatgptApiSpeechFormat('wav')`)).toThrow('unsupported_format')
    expect(evaluate(`chatgptApiSpeechFormat('opus')`)).toEqual({
      requested_format: 'opus',
      actual_format: 'opus',
    })
    expect(() => vm.runInContext(`chatgptApiSpeechVoice('unknown')`, context)).toThrow(
      'unsupported_voice'
    )
    expect(() => vm.runInContext(`chatgptApiSpeechFormat('ogg')`, context)).toThrow(
      'unsupported_format'
    )
  })

  it('完整消息之后的增量文本保留消息编号，并核对最终朗读原文', () => {
    const events = [
      {
        v: {
          conversation_id: 'speech-stream',
          message: {
            id: 'assistant-stream',
            author: { role: 'assistant' },
            content: { parts: [''] },
          },
        },
      },
      { p: '/message/content/parts/0', o: 'append', v: '你好，' },
      { p: '/message/content/parts/0', o: 'append', v: '模型连接正常。' },
    ]
    expect(
      evaluate(`chatgptApiSpeechMessageIds(__events, '你好，模型连接正常。')`, {
        __events: events,
      })
    ).toEqual({ conversation_id: 'speech-stream', message_id: 'assistant-stream' })
    expect(
      evaluate(`chatgptApiSpeechMessageIds(__events, '你好，模型连接正常。')`, {
        __events: [
          ...events,
          { p: '/message/content/parts/0', o: 'append', v: '还有什么需要帮助？' },
        ],
      })
    ).toEqual({ conversation_id: 'speech-stream', message_id: '' })
  })

  it('复用网页文本提交，再朗读已确认逐字一致的回复', () => {
    vm.runInContext(
      `
      globalThis.__requests = []
      host.stream = function (_action, request, onEvent) {
        globalThis.__requests.push({ method: request.method, path: '/backend-api/f/conversation', options: { body: request.body } })
        onEvent({ v: { conversation_id: 'conversation-tts', message: {
          id: 'message-tts', author: { role: 'assistant' }, content: { parts: ['需要朗读的台词'] }
        } } })
        return { ok: true }
      }
      chatgptApiInitializeConversation = function () { return { default_model_slug: 'auto' } }
      chatgptApiSentinelHeaders = function () { return { 'openai-sentinel-chat-requirements-token': 'sentinel' } }
      chatgptApiRequest = function (_auth, method, requestPath, options) {
        globalThis.__requests.push({ method: method, path: requestPath, options: options || {} })
        if (requestPath.endsWith('/prepare')) {
          return { ok: true, body: { status: 'ok', conduit_token: 'conduit-' + globalThis.__requests.length } }
        }
        if (method === 'GET' && requestPath.startsWith('/backend-api/synthesize?')) {
          return { ok: true, headers: { 'content-type': 'audio/ogg; charset=binary' }, body_base64: 'T2dnUw==' }
        }
        return { ok: true, body: {} }
      }
      `,
      context
    )
    const auth = {
      cookies: [],
      runtime_started_at: Date.now(),
      auth: { fingerprint: { timezone: 'Asia/Shanghai', timezone_offset_min: -480 } },
    }
    const result = evaluate(`chatgptApiGenerateSpeech(__auth, '需要朗读的台词', 'nova', 'opus')`, {
      __auth: auth,
    })
    const requests = evaluate('globalThis.__requests')
    expect(result).toMatchObject({
      audio_base64: 'T2dnUw==',
      mime_type: 'audio/ogg',
      format: 'opus',
      requested_format: 'opus',
      voice: 'nova',
      platform_voice: 'maple',
      message_id: 'message-tts',
      conversation_id: 'conversation-tts',
    })
    const prepare = requests.filter((request) => request.path.endsWith('/prepare'))
    expect(prepare.map((request) => request.options.body.client_prepare_state)).toEqual(['sent'])
    expect(prepare[0].options.body.partial_query.content.parts[0]).toContain('需要朗读的台词')
    const sent = requests.find((request) => request.path === '/backend-api/f/conversation')
    expect(sent.options.body.messages[0].author.role).toBe('user')
    const synthesize = requests.find((request) =>
      request.path.startsWith('/backend-api/synthesize?')
    )
    expect(synthesize.options.response_body_format).toBe('base64')
    expect(synthesize.path).toContain('message_id=message-tts')
    expect(synthesize.path).toContain('conversation_id=conversation-tts')
    expect(synthesize.path).toContain('voice=maple')
    expect(synthesize.path).toContain('format=opus')
    expect(requests.at(-1)).toMatchObject({
      method: 'PATCH',
      path: '/backend-api/conversation/conversation-tts',
    })
  })

  it('识别批量 SSE patch，并拒绝空输入、变速和朗读风格指令', () => {
    const ids = evaluate(`chatgptApiSpeechMessageIds(__events, '台词')`, {
      __events: [
        { p: '/conversation_id', o: 'replace', v: 'conversation-patch' },
        { p: '/message/id', o: 'replace', v: 'message-patch' },
        { p: '/message/author/role', o: 'replace', v: 'assistant' },
        { p: '/message/content/parts/0', o: 'append', v: '台' },
        { p: '', o: 'patch', v: [{ p: '/message/content/parts/0', o: 'append', v: '词' }] },
      ],
    })
    expect(ids).toEqual({ conversation_id: 'conversation-patch', message_id: 'message-patch' })
    expect(() =>
      vm.runInContext(`main({ workspace_dir: '/tmp/tts-test', request: { input: '' } })`, context)
    ).toThrow('missing_input')
    expect(() =>
      vm.runInContext(
        `main({ workspace_dir: '/tmp/tts-test', request: { input: '台词', speed: 1.25 } })`,
        context
      )
    ).toThrow('invalid_input')
    expect(() =>
      vm.runInContext(
        `main({ workspace_dir: '/tmp/tts-test', request: { input: '台词', instructions: '激动地朗读' } })`,
        context
      )
    ).toThrow('invalid_input')
  })

  it('JSON 下载地址会继续下载签名音频，并按实际格式返回 MIME', () => {
    vm.runInContext(
      `
      chatgptApiBase64DecodeBinary = function () {
        return JSON.stringify({ download_url: 'https://blob.core.windows.net/audio/speech.aac' })
      }
      host.call = function (action, request) {
        globalThis.__download = { action: action, request: request }
        return { ok: true, headers: {}, body_base64: 'QUFD' }
      }
      `,
      context
    )
    const result = evaluate(`chatgptApiResolveSpeechResponse({}, __response, 'aac')`, {
      __response: {
        headers: { 'content-type': 'application/json' },
        body_base64:
          'eyJkb3dubG9hZF91cmwiOiJodHRwczovL2Jsb2IuY29yZS53aW5kb3dzLm5ldC9hdWRpby9zcGVlY2guYWFjIn0=',
      },
    })
    expect(result).toEqual({ audio_base64: 'QUFD', mime_type: 'audio/aac' })
    expect(evaluate('globalThis.__download')).toMatchObject({
      action: 'http.request',
      request: {
        url: 'https://blob.core.windows.net/audio/speech.aac',
        response_body_format: 'base64',
      },
    })
  })
})

import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import { expect, it, vi } from 'vitest'

const source = [
  '../../../plugins/chatgpt/scripts/network_profile.js',
  '../../../plugins/chatgpt/scripts/auth.js',
  '../../../plugins/chatgpt/scripts/poll_retry.js',
]
  .map((file) =>
    readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), file), 'utf8')
  )
  .join('\n')

it('接续 Conduit 原会话并立即保存恢复凭据，不重复提交用户消息', () => {
  let saved = { phase: 'submitting', model: 'gpt-6-astra-wm' }
  const events = []
  const calls = []
  const checkpoint = vi.fn((value) => {
    if (value) saved = value
    return saved
  })
  const context = vm.createContext({
    chatgptApiTlsProfile: () => 'chrome_149',
    chatgptApiCheckpoint: checkpoint,
    receive: (event) => events.push(event),
    host: {
      stream: (_action, request, callback) => {
        calls.push(request)
        if (calls.length === 1) {
          callback({
            type: 'resume_conversation_token',
            conversation_id: 'conversation-1',
            token: 'resume-test',
          })
          callback({ type: 'stream_handoff' })
        } else {
          callback({ type: 'stream_handoff' })
          callback({ v: { message: { content: { parts: ['OK'] } } } })
          callback({ v: '!' })
          callback({ type: 'message_stream_complete' })
        }
        return { ok: true, status: 200 }
      },
    },
  })
  vm.runInContext(source, context)
  context.chatgptApiBaseHeaders = () => ({ authorization: 'test-only' })
  vm.runInContext(
    `chatgptApiStreamWithResume({}, {url:'https://chatgpt.com/backend-api/f/conversation',body:{messages:['original']}}, receive)`,
    context
  )
  expect(calls).toHaveLength(2)
  expect(calls[1].url).toBe('https://chatgpt.com/backend-api/f/conversation/resume')
  expect(calls[1].body).toEqual({ conversation_id: 'conversation-1', offset: 0 })
  expect(calls[1].headers['x-conduit-token']).toBe('resume-test')
  expect(saved).toEqual({
    phase: 'accepted',
    model: 'gpt-6-astra-wm',
    conversation_id: 'conversation-1',
    resume_token: 'resume-test',
    resume_kind: '',
  })
  expect(checkpoint).toHaveBeenCalledTimes(2)
  expect(events).toHaveLength(6)
})

it('接续失败保留上游状态，不再次发送生成请求', () => {
  const stream = vi.fn((_action, _request, callback) => {
    callback({ type: 'stream_handoff' })
    return { ok: false, status: 429 }
  })
  const context = vm.createContext({
    chatgptApiTlsProfile: () => 'chrome_149',
    chatgptApiCheckpoint: () => ({ phase: 'accepted', conversation_id: 'original' }),
    host: { stream },
  })
  vm.runInContext(source, context)
  const result = vm.runInContext(
    `chatgptApiStreamWithResume({}, {url:'https://chatgpt.com/backend-api/f/conversation'}, () => {})`,
    context
  )
  expect(result.status).toBe(429)
  expect(stream).toHaveBeenCalledTimes(1)
})

it('缺失恢复凭据时明确报错，不能把转交当作成功', () => {
  const context = vm.createContext({
    chatgptApiTlsProfile: () => 'chrome_149',
    host: {
      stream: (_action, _request, callback) => {
        callback({ type: 'stream_handoff' })
        return { ok: true, status: 200 }
      },
    },
  })
  vm.runInContext(source, context)
  expect(() =>
    vm.runInContext(
      `chatgptApiStreamWithResume({}, {url:'https://chatgpt.com/backend-api/f/conversation'}, () => {})`,
      context
    )
  ).toThrow('PROVIDER_SUBMISSION_UNKNOWN')
})

import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const scriptDirectory = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../../plugins/chatgpt/scripts'
)

let context
let checkpoint
let writes
let stream
let poll
let refresh

beforeEach(() => {
  checkpoint = {}
  writes = []
  stream = vi.fn(() => {
    throw new Error('connection reset')
  })
  poll = vi.fn(() => '原会话的完整结果')
  refresh = vi.fn((auth) => auth)
  context = vm.createContext({
    host: {
      call: () => ({}),
      emit: () => {},
      checkpoint: (value) => {
        if (value !== undefined) {
          checkpoint = JSON.parse(JSON.stringify(value))
          writes.push(checkpoint)
        }
        return checkpoint
      },
    },
  })
  for (const name of [
    'auth',
    'conversation',
    'conversation_results',
    '../../_shared/api_text_attachments',
    '../../_shared/api_text_responses',
    'text_conversation',
    'generate_text',
  ]) {
    vm.runInContext(readFileSync(path.join(scriptDirectory, `${name}.js`), 'utf8'), context)
  }
  Object.assign(context, {
    chatgptApiReadAuth: () => ({}),
    chatgptApiRequest: () => ({ body: { models: [{ slug: 'gpt-5-6-instant' }] } }),
    chatgptApiRefreshSession: refresh,
    chatgptApiInitializeConversation: vi.fn(() => ({ default_model_slug: 'auto' })),
    chatgptApiSentinelHeaders: () => ({}),
    chatgptApiPrepareTextConversation: (_auth, _prompt, thread, options) => ({
      ...thread,
      ...options,
      user_message_id: 'user-1',
    }),
    chatgptApiStreamTextConversation: stream,
    chatgptApiWaitForText: poll,
    chatgptApiHideConversation: vi.fn(),
  })
})

const run = () =>
  vm.runInContext(`main({ request: { input: '提取资产', model: 'gpt-5-6-instant' } })`, context)

describe('ChatGPT 文本提交与恢复', () => {
  it('清理远端会话前获取标题，标题和正文一起持久化并可恢复', () => {
    checkpoint = { phase: 'accepted', conversation_id: 'original' }
    context.chatgptApiFetchTextConversation = vi.fn(() => ({ title: '杯子广告' }))
    context.chatgptApiHideConversation.mockImplementation(() => {
      expect(checkpoint.result.response.metadata).toEqual({
        conversation_id: 'original',
        conversation_title: '杯子广告',
      })
    })
    const result = run()
    expect(result.response.output_text).toBe('原会话的完整结果')
    expect(context.chatgptApiFetchTextConversation).toHaveBeenCalledWith({}, 'original', 3000)
    expect(run()).toEqual(result)
    expect(context.chatgptApiFetchTextConversation).toHaveBeenCalledTimes(1)
  })

  it('文本提交边界由实际发送前的检查点控制，不覆盖整个认证和准备步骤', () => {
    const manifest = JSON.parse(
      readFileSync(path.join(scriptDirectory, '../manifest.json'), 'utf8')
    )
    expect(manifest.flows.generate_text.steps[0].submission_boundary).not.toBe(true)
  })

  it('认证和准备阶段失败不标记为已提交，允许安全重试或换号', () => {
    refresh.mockImplementationOnce(() => {
      throw new Error('temporary_unavailable: auth')
    })
    expect(run).toThrow('auth')
    expect(checkpoint.phase).toBe('ready')
    context.chatgptApiPrepareTextConversation = () => {
      throw new Error('temporary_unavailable: prepare')
    }
    expect(run).toThrow('prepare')
    expect(checkpoint.phase).toBe('ready')
    expect(stream).not.toHaveBeenCalled()
  })

  it('发送前保存提交标记，无会话编号的断线不会自动重发', () => {
    stream.mockImplementation(() => {
      expect(checkpoint.phase).toBe('submitting')
      throw new Error('connection reset')
    })
    expect(run).toThrow('connection reset')
    expect(run).toThrow('PROVIDER_SUBMISSION_UNKNOWN')
    expect(stream).toHaveBeenCalledTimes(1)
  })

  it('流中立即保存会话编号，断线后只读取原会话并保存最终正文', () => {
    stream.mockImplementation((_auth, _prompt, _prepared, _headers, _attachments, onEvent) => {
      onEvent({ p: '/conversation_id', o: 'replace', v: 'conversation-original' })
      expect(checkpoint.conversation_id).toBe('conversation-original')
      expect(checkpoint.phase).toBe('accepted')
      throw new Error('connection reset')
    })
    expect(run).toThrow('connection reset')
    const result = run()
    expect(result.response.output_text).toBe('原会话的完整结果')
    expect(poll).toHaveBeenCalledWith({}, 'conversation-original', 'client-created-root')
    expect(stream).toHaveBeenCalledTimes(1)
    expect(context.chatgptApiInitializeConversation).toHaveBeenCalledTimes(1)
    expect(checkpoint.phase).toBe('completed')
    expect(checkpoint.result.response.output_text).toBe('原会话的完整结果')
  })

  it('前端流事件投递失败前已保存同一事件中的会话编号', () => {
    context.host.emit = (event) => {
      if (event.type === 'response.output_text.delta') throw new Error('event delivery failed')
    }
    stream.mockImplementation((_auth, _prompt, _prepared, _headers, _attachments, onEvent) => {
      onEvent({
        v: {
          conversation_id: 'event-original',
          message: {
            id: 'message-1',
            author: { role: 'assistant' },
            channel: 'final',
            content: { parts: ['完整文本'] },
            end_turn: true,
          },
        },
      })
    })
    expect(run).toThrow('event delivery failed')
    expect(checkpoint.phase).toBe('accepted')
    expect(checkpoint.conversation_id).toBe('event-original')
  })

  it('清理远端会话失败后仍可直接交付已保存结果，不再认证和请求平台', () => {
    checkpoint = { phase: 'accepted', conversation_id: 'original', model: 'auto' }
    context.chatgptApiHideConversation.mockImplementationOnce(() => {
      throw new Error('cleanup failed')
    })
    expect(run).toThrow('cleanup failed')
    const result = run()
    expect(result.response.output_text).toBe('原会话的完整结果')
    expect(refresh).toHaveBeenCalledTimes(1)
    expect(poll).toHaveBeenCalledTimes(1)
    expect(stream).not.toHaveBeenCalled()
  })

  it('保存提交凭据失败时不发送平台请求', () => {
    context.host.checkpoint = (value) => {
      if (value?.phase === 'submitting') throw new Error('disk full')
      return {}
    }
    expect(run).toThrow('disk full')
    expect(stream).not.toHaveBeenCalled()
  })

  it('支持手动关联旧文本会话，缺少旧检查点字段也只读取结果', () => {
    checkpoint = { phase: 'accepted', conversation_id: 'manually-linked' }
    expect(run().response.output_text).toBe('原会话的完整结果')
    expect(stream).not.toHaveBeenCalled()
    expect(context.chatgptApiInitializeConversation).not.toHaveBeenCalled()
  })
})

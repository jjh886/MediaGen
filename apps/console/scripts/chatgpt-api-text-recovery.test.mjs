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
    'network_profile',
    'conversation',
    'conversation_results',
    '../../_shared/api_text_attachments',
    '../../_shared/api_text_responses',
    'poll_retry',
    '../../_shared/api_text_media',
    'text_updates',
    'text_media',
    'text_conversation',
    'generate_text',
  ]) {
    vm.runInContext(readFileSync(path.join(scriptDirectory, `${name}.js`), 'utf8'), context)
  }
  context.actualWaitForText = context.chatgptApiWaitForText
  Object.assign(context, {
    chatgptApiReadAuth: () => ({}),
    chatgptApiBootstrap: () => ({
      '/backend-api/models': { models: [{ slug: 'gpt-5-6-instant' }] },
    }),
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
    chatgptApiWaitForRichText: poll,
    chatgptApiOpenTextUpdates: () => ({}),
    chatgptApiCloseTextUpdates: () => {},
    chatgptApiResumeRequest: () => ({}),
    chatgptApiHideConversation: vi.fn(),
  })
})

const run = () =>
  vm.runInContext(`main({ request: { input: '提取资产', model: 'gpt-5-6-instant' } })`, context)

describe('ChatGPT 文本提交与恢复', () => {
  it('标题和正文一起持久化并可恢复，保留平台会话', () => {
    checkpoint = { phase: 'accepted', conversation_id: 'original' }
    context.chatgptApiFetchTextConversation = vi.fn(() => ({ title: '杯子广告' }))
    const result = run()
    expect(result.response.output_text).toBe('原会话的完整结果')
    expect(checkpoint.result.response.metadata).toEqual({
      conversation_id: 'original',
      conversation_title: '杯子广告',
    })
    expect(context.chatgptApiHideConversation).not.toHaveBeenCalled()
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
    expect(poll).toHaveBeenCalledWith(
      {},
      'conversation-original',
      'client-created-root',
      expect.any(Object),
      expect.any(Object),
      expect.any(Object)
    )
    expect(stream).toHaveBeenCalledTimes(1)
    expect(context.chatgptApiInitializeConversation).toHaveBeenCalledTimes(1)
    expect(checkpoint.phase).toBe('completed')
    expect(checkpoint.result.response.output_text).toBe('原会话的完整结果')
  })

  it('已接收会话的恢复流过期后轮询同一会话，不重复提交', () => {
    checkpoint = { phase: 'accepted', conversation_id: 'original', resume_token: 'expired' }
    context.chatgptApiStreamWithResume = vi.fn(() => ({
      ok: false,
      status: 404,
      body: { detail: 'Resume stream unavailable' },
    }))
    expect(run().response.output_text).toBe('原会话的完整结果')
    expect(poll).toHaveBeenCalledWith(
      {},
      'original',
      'client-created-root',
      expect.any(Object),
      expect.any(Object),
      expect.any(Object)
    )
    expect(stream).not.toHaveBeenCalled()
    expect(writes.some((value) => value.resume_token === '')).toBe(true)
  })

  it('首次发送已取得会话编号但接续流过期时仍找回同一会话', () => {
    stream.mockImplementation((_auth, _prompt, _prepared, _headers, _attachments, onEvent) => {
      onEvent({ p: '/conversation_id', o: 'replace', v: 'original' })
      return { ok: false, status: 404 }
    })
    expect(run().response.output_text).toBe('原会话的完整结果')
    expect(poll).toHaveBeenCalledWith(
      {},
      'original',
      'client-created-root',
      expect.any(Object),
      expect.any(Object),
      expect.any(Object)
    )
    expect(stream).toHaveBeenCalledTimes(1)
  })

  it('恢复时认证失败仍明确报错，不吞掉账号问题', () => {
    checkpoint = { phase: 'accepted', conversation_id: 'original', resume_token: 'expired' }
    context.chatgptApiStreamWithResume = () => ({ ok: false, status: 401 })
    expect(run).toThrow('LOGIN_EXPIRED')
    expect(poll).not.toHaveBeenCalled()
  })

  it('无会话编号的发送错误不能视作已接收或重新提交', () => {
    stream.mockReturnValue({ ok: false, status: 404 })
    expect(run).toThrow('HTTP 404')
    expect(run).toThrow('PROVIDER_SUBMISSION_UNKNOWN')
    expect(poll).not.toHaveBeenCalled()
    expect(stream).toHaveBeenCalledTimes(1)
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

  it('已完成结果不再认证和请求平台，也不发送隐藏会话请求', () => {
    checkpoint = { phase: 'accepted', conversation_id: 'original', model: 'auto' }
    context.chatgptApiHideConversation.mockImplementationOnce(() => {
      throw new Error('cleanup failed')
    })
    const result = run()
    expect(run()).toEqual(result)
    expect(result.response.output_text).toBe('原会话的完整结果')
    expect(refresh).toHaveBeenCalledTimes(1)
    expect(poll).toHaveBeenCalledTimes(1)
    expect(stream).not.toHaveBeenCalled()
    expect(context.chatgptApiHideConversation).not.toHaveBeenCalled()
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

  it('查询已受理会话被限流时退避，继续读取原结果而不重发', () => {
    context.chatgptApiWaitForText = context.actualWaitForText
    const request = vi
      .fn()
      .mockReturnValueOnce({ status: 429, headers: { 'retry-after': '45' } })
      .mockReturnValueOnce({ status: 429, headers: {} })
      .mockReturnValueOnce({ ok: true, status: 200, body: { pending: true } })
      .mockReturnValueOnce({ ok: true, status: 200, body: { complete: true } })
    const sleep = vi.fn()
    context.chatgptApiRequest = request
    context.chatgptApiFinalTextFromConversation = (body) => (body.complete ? '完整稿件' : '')
    context.host.call = sleep
    expect(vm.runInContext("chatgptApiWaitForText({}, 'original', 'baseline')", context)).toBe(
      '完整稿件'
    )
    expect(sleep.mock.calls.map((call) => call[1].milliseconds)).toEqual([45000, 90000, 30000])
    expect(
      request.mock.calls.every((call) => call[1] === 'GET' && call[2].endsWith('/original'))
    ).toBe(true)
    expect(stream).not.toHaveBeenCalled()
  })

  it('遵守大小写不固定的 Retry-After，不把服务要求的等待截断为一分钟', () => {
    expect(context.chatgptApiPollRetryDelayMs({ headers: { 'Retry-After': '600' } }, 30000)).toBe(
      600000
    )
    const retryDate = new Date(Date.now() + 600000).toUTCString()
    expect(
      context.chatgptApiPollRetryDelayMs({ headers: { 'RETRY-AFTER': retryDate } }, 30000)
    ).toBeGreaterThan(599000)
    expect(context.chatgptApiPollRetryDelayMs({ headers: {} }, 240000)).toBe(300000)
  })

  it('恢复同一会话保留限流等待，成功查询后恢复正常频率并保留提交身份', () => {
    vi.useFakeTimers()
    try {
      const now = Date.now()
      context.Date = Date
      checkpoint = {
        phase: 'accepted',
        conversation_id: 'original',
        user_message_id: 'user-1',
        poll_retry_ms: 240000,
        poll_next_at_ms: now + 120000,
      }
      context.chatgptApiRequest = vi
        .fn()
        .mockReturnValueOnce({ status: 429, headers: {} })
        .mockReturnValueOnce({ ok: true, status: 200, body: { complete: true } })
      context.chatgptApiFinalTextFromConversation = (body) => (body.complete ? '完整稿件' : '')
      const sleep = vi.fn((_name, input) => vi.advanceTimersByTime(input.milliseconds))
      context.host.call = sleep
      expect(vm.runInContext("actualWaitForText({}, 'original', 'baseline')", context)).toBe(
        '完整稿件'
      )
      expect(sleep.mock.calls.map((call) => call[1].milliseconds)).toEqual([120000, 300000])
      expect(checkpoint).toMatchObject({
        phase: 'accepted',
        conversation_id: 'original',
        user_message_id: 'user-1',
        poll_retry_ms: 30000,
        poll_next_at_ms: 0,
      })
      expect(writes.some((value) => value.poll_retry_ms === 300000)).toBe(true)
      expect(stream).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('不沿用或覆盖其他会话的限流检查点', () => {
    checkpoint = {
      phase: 'accepted',
      conversation_id: 'other',
      poll_retry_ms: 600000,
      poll_next_at_ms: Date.now() + 600000,
    }
    context.chatgptApiRequest = () => ({ ok: true, status: 200, body: {} })
    context.chatgptApiFinalTextFromConversation = () => '完整稿件'
    const sleep = vi.fn()
    context.host.call = sleep
    expect(vm.runInContext("actualWaitForText({}, 'original', 'baseline')", context)).toBe(
      '完整稿件'
    )
    expect(sleep).not.toHaveBeenCalled()
    expect(checkpoint.conversation_id).toBe('other')
    expect(writes).toEqual([])
  })

  it('查询原会话遇到认证错误仍明确失败，不将所有失败隐藏为限流', () => {
    context.chatgptApiWaitForText = context.actualWaitForText
    context.chatgptApiRequest = () => ({ ok: false, status: 401 })
    expect(() =>
      vm.runInContext("chatgptApiWaitForText({}, 'original', 'baseline')", context)
    ).toThrow('401')
  })
})

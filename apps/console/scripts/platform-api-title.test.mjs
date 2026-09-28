import { describe, expect, it, vi } from 'vitest'
import { scripts } from './platform-api-test-helper.mjs'

const shared = '_shared/api_text_responses.js'

describe('平台会话标题', () => {
  it('等待默认标题更新，完成事件和最终返回值携带同一标题，不混入正文', () => {
    const emitted = []
    const call = vi.fn()
    const run = scripts([shared], { call, emit: (event) => emitted.push(event) })
    const read = vi.fn().mockReturnValueOnce('New chat').mockReturnValue(' 橙色杯子\n广告 ')
    const response = run(
      `
      const writer = apiTextCreateResponseWriter({model:'model',input:[],tools:[]});
      apiTextFeedResponseText(writer, '这是正文');
      apiTextReadConversationTitle(writer, 'thread', read);
      apiTextCompleteResponse(writer)
    `,
      { read }
    )
    expect(response.output_text).toBe('这是正文')
    expect(response.metadata).toEqual({
      conversation_id: 'thread',
      conversation_title: '橙色杯子 广告',
    })
    expect(emitted.at(-1).response.metadata).toEqual(response.metadata)
    expect(read).toHaveBeenCalledTimes(2)
    expect(call).toHaveBeenCalledWith('time.sleep', { milliseconds: 500 })
  })

  it('标题失败、空标题和无效类型不破坏正文，并限制轮询次数和 Unicode 长度', () => {
    const run = scripts([shared], { call: vi.fn(), emit: vi.fn() })
    for (const value of ['', '新对话', null, { title: '无效类型' }]) {
      const read = vi.fn(() => value)
      const result = run(
        `(() => {
        const writer = apiTextCreateResponseWriter({model:'model',tools:[]});
        apiTextFeedResponseText(writer, '正文');
        apiTextReadConversationTitle(writer, 'id', read);
        return apiTextCompleteResponse(writer);
      })()`,
        { read }
      )
      expect(result.output_text).toBe('正文')
      expect(result.metadata).toEqual({})
      expect(read.mock.calls.length).toBeLessThanOrEqual(3)
    }
    const writer = {}
    expect(() =>
      run('apiTextReadConversationTitle(writer, "id", read)', {
        writer,
        read: () => {
          throw new Error('LOGIN_EXPIRED')
        },
      })
    ).not.toThrow()
    run('apiTextReadConversationTitle(writer, "id", () => "🎬".repeat(121))', { writer })
    expect(Array.from(writer.metadata.conversation_title)).toHaveLength(120)
  })

  it.each(['doubao', 'deepseek'])(
    '%s 正文完成后只读取原会话标题，恢复结果不重复请求',
    (platform) => {
      let saved =
        platform === 'doubao'
          ? { conversation_id: '123' }
          : { conversation_id: '123', response_message_id: 7 }
      const queries = []
      const run = scripts([shared, `${platform}-api/scripts/generate_text.js`], {
        checkpoint: (value) => {
          if (value) saved = value
          return saved
        },
        emit: vi.fn(),
        call: vi.fn(),
      })
      const response = run(`main({request:{input:'测试',model:'model'}})`, {
        apiTextResponsesMedia: () => [],
        apiSessionRead: () => ({}),
        doubaoRequireVerified: () => {},
        doubaoWaitForResult: () => '正文',
        doubaoMarkConversationRead: () => {},
        doubaoImRequest: (...args) => {
          queries.push(args)
          return {
            downlink_body: {
              get_conv_info_downlink_body: {
                conversation_info: { conversation_id: '123', name: '杯子广告' },
              },
            },
          }
        },
        deepseekValidateAttachments: () => {},
        deepseekStreamState: () => ({}),
        deepseekSnapshot: (state, writer) => {
          state.finished = true
          run('apiTextFeedResponseText(writer, "正文")', { writer })
        },
        deepseekRequest: (...args) => {
          queries.push(args)
          return {
            chat_session: { id: '123', title: '杯子广告' },
            chat_messages: [{ message_id: 7, role: 'ASSISTANT' }],
          }
        },
      })
      expect(response.response.metadata.conversation_title).toBe('杯子广告')
      expect(queries.at(-1).at(-1)).toBe(3000)
      const count = queries.length
      expect(run('main({request:{input:"测试",model:"model"}})')).toEqual(response)
      expect(queries).toHaveLength(count)
    }
  )
})

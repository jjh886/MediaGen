import { describe, expect, it } from 'vitest'
import { scripts } from './platform-api-test-helper.mjs'

describe('豆包额度限制与旧文本回复', () => {
  it('按真实商业限制元数据报告额度耗尽，不把恢复提示当作生成结果', () => {
    const run = scripts(['doubao-api/scripts/auth.js'])
    const reset = Date.now() + 60000
    const content = {
      ext: {
        not_in_chat_context: 'commerce_intake_block',
        inner_paywall_cta_param: JSON.stringify({
          snapshot: JSON.stringify({
            remaining_free_quota: false,
            unavailable_reason: { type: 2, window_limit_type: '7d', reset_at: reset },
          }),
        }),
      },
    }
    expect(() => run('doubaoCheckCommerce(content)', { content })).toThrow('quota_exhausted:')
    expect(() => run('doubaoCheckCommerce(content)')).toThrow(/retry after \d+ seconds/)
    expect(() =>
      run('doubaoCheckCommerce({ext:{inner_paywall_cta_param:"invalid"}})')
    ).not.toThrow()
    expect(() =>
      run(
        'doubaoCheckCommerce({ext:{not_in_chat_context:"commerce_intake_block",inner_paywall_cta_param:"invalid"}})'
      )
    ).toThrow('invalid_response:')
  })

  it('图片识别的旧文本协议交付正文，忽略用户回显和其他消息，完成快照不重复输出', () => {
    const output = []
    const run = scripts(
      [
        '_shared/api_session.js',
        'doubao-api/scripts/auth.js',
        'doubao-api/scripts/conversation.js',
      ],
      { checkpoint: () => ({}) }
    )
    run('state=doubaoStreamState();onText=callback', { callback: (text) => output.push(text) })
    const notify = (userType, messageId, text) =>
      run('doubaoConsume(state,event,onText)', {
        event: {
          event: 'STREAM_MSG_NOTIFY',
          data: {
            meta: { user_type: userType, message_id: messageId },
            content: { content_type: 1, content: JSON.stringify({ text }) },
          },
        },
      })
    const patch = (messageId, type, text) =>
      run('doubaoConsume(state,event,onText)', {
        event: {
          event: 'STREAM_CHUNK',
          data: {
            message_id: messageId,
            patch_op: [
              {
                patch_object: 102,
                patch_type: type,
                patch_value: { content: JSON.stringify({ text }) },
              },
            ],
          },
        },
      })
    notify(1, 'user', '用户的问题')
    notify(2, 'answer', 'IMAGE_')
    patch('other', 1, '错误消息')
    patch('answer', 1, 'OK_4271')
    patch('answer', 2, 'IMAGE_OK_4271')
    expect(output.join('')).toBe('IMAGE_OK_4271')
    expect(run('state.text')).toBe('IMAGE_OK_4271')
    expect(() => patch('answer', 2, '改写的正文')).toThrow('generation_incomplete:')
  })

  it('流式和恢复回读遇到额度耗尽都立即失败，不交付限制提示或旧结果', () => {
    const run = scripts(
      [
        '_shared/api_session.js',
        'doubao-api/scripts/auth.js',
        'doubao-api/scripts/conversation.js',
        'doubao-api/scripts/history.js',
      ],
      {
        checkpoint: () => ({}),
        sleep: () => {
          throw new Error('不应继续等待')
        },
      }
    )
    const restricted = {
      user_type: 2,
      index_in_conv: 2,
      ext: {
        not_in_chat_context: 'commerce_intake_block',
        inner_paywall_cta_param: JSON.stringify({ snapshot: { unavailable_reason: { type: 2 } } }),
      },
    }
    const output = []
    expect(() =>
      run('doubaoConsume(doubaoStreamState(),event,onText)', {
        event: { event: 'STREAM_MSG_NOTIFY', data: { content: restricted } },
        onText: (text) => output.push(text),
      })
    ).toThrow('quota_exhausted:')
    expect(output).toEqual([])
    run('doubaoHistory=()=>messages', {
      messages: [
        {
          user_type: 2,
          index_in_conv: 1,
          ext: { is_finish: '1' },
          content_type: 1,
          content: JSON.stringify({ text: '旧回复' }),
        },
        restricted,
      ],
    })
    for (const kind of ['text', 'image', 'video'])
      expect(() => run('doubaoWaitForResult({},"123",kind)', { kind })).toThrow('quota_exhausted:')
  })

  it('从原会话恢复旧协议正文，旧的商业提示不阻断最新回复', () => {
    const run = scripts([
      'doubao-api/scripts/auth.js',
      'doubao-api/scripts/conversation.js',
      'doubao-api/scripts/history.js',
    ])
    run('doubaoHistory=()=>messages', {
      messages: [
        {
          user_type: 2,
          index_in_conv: 1,
          ext: { not_in_chat_context: 'commerce_intake_block', inner_paywall_cta_param: 'invalid' },
        },
        {
          user_type: 2,
          index_in_conv: 2,
          ext: { is_finish: '1' },
          content_type: 1,
          content: JSON.stringify({ text: '图片里有中文：427' }),
        },
      ],
    })
    expect(run('doubaoWaitForResult({},"123","text")')).toBe('图片里有中文：427')
    expect(() => run('doubaoLegacyText({content_type:1,content:"invalid"})')).toThrow(
      'invalid_response:'
    )
  })

  it('订阅权限不足保留业务失败，不把它判成登录失效或虚报成功', () => {
    const run = scripts(['doubao-api/scripts/auth.js'])
    expect(() =>
      run('doubaoCheckCommerce(content)', {
        content: {
          ext: {
            not_in_chat_context: 'commerce_intake_block',
            inner_paywall_cta_param: JSON.stringify({
              snapshot: { unavailable_reason: { type: 1 } },
            }),
          },
        },
      })
    ).toThrow('platform_rejected:')
    expect(() =>
      run('doubaoCheckCommerce(content)', {
        content: {
          ext: {
            not_in_chat_context: 'commerce_intake_block',
            inner_paywall_cta_param: 'x'.repeat(16385),
          },
        },
      })
    ).toThrow('invalid_response:')
  })

  it('视频下载地址等待期间执行账号续期，始终查找原视频', () => {
    const requests = []
    let elapsed = 0
    let reads = 0
    const run = scripts(
      [
        'doubao-api/scripts/auth.js',
        'doubao-api/scripts/passport.js',
        'doubao-api/scripts/history.js',
      ],
      {
        sleep: (milliseconds) => {
          elapsed += milliseconds
        },
      }
    )
    run('Date.now=clock;doubaoRequest=request', {
      clock: () => elapsed,
      request: (_session, path, body) => {
        requests.push({ path, body })
        if (path.endsWith('/homepage'))
          return ++reads < 3 ? { children: [] } : { children: [{ name: '我的创作', id: 'folder' }] }
        if (path.endsWith('/node_info'))
          return { children: [{ key: 'original-video', id: 'node' }] }
        if (path.endsWith('/get_download_info'))
          return { download_infos: [{ main_url: 'https://media.example/original.mp4' }] }
        if (path.startsWith('/passport/')) return {}
        throw new Error('不能重新生成视频')
      },
    })
    expect(run('doubaoVideoDownload({heartbeatState:{token:0,security:0}},"original-video")')).toBe(
      'https://media.example/original.mp4'
    )
    expect(elapsed).toBe(10000)
    expect(
      requests.filter(({ path }) => path.startsWith('/passport/')).map(({ path }) => path)
    ).toEqual(['/passport/token/beat/web/', '/passport/doubao/security_token_beat/'])
  })
})

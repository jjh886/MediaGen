import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import { describe, expect, it } from 'vitest'

function runtime() {
  const manifest = JSON.parse(readFileSync('../../plugins/chatgpt/manifest.json', 'utf8'))
  const events = []
  let checkpoint = { user_message_id: 'user-new' }
  const context = vm.createContext({
    host: {
      emit: (event) => events.push(JSON.parse(JSON.stringify(event))),
      checkpoint: (value) => {
        if (value) checkpoint = value
        return checkpoint
      },
      call: () => ({}),
      tryCall: () => ({ success: true }),
    },
  })
  for (const file of manifest.flows.generate_text.steps[0].script_files) {
    const path = file.startsWith('@shared/')
      ? '../../plugins/_shared/' + file.slice(8)
      : '../../plugins/chatgpt/scripts/' + file
    vm.runInContext(readFileSync(path, 'utf8'), context)
  }
  vm.runInContext(
    "var state=chatgptApiCreateTextStreamState(); var writer=apiTextCreateResponseWriter(apiTextNormalizeTextRequest({input:'生成一张图片',model:'gpt-6-pro'})); apiTextStartResponse(writer)",
    context
  )
  return {
    context,
    events,
    run(code, values = {}) {
      Object.assign(context, values)
      return vm.runInContext(code, context)
    },
  }
}
function message(id, content, metadata = {}, role = 'assistant') {
  return {
    id,
    author: { role },
    content,
    metadata,
    status: 'finished_successfully',
    channel: 'final',
  }
}

describe('ChatGPT 对话中的思考和图片', () => {
  it('仅有思考状态或耗时不产生摘要，结束回执不覆盖真实内容', () => {
    const { run, events } = runtime()
    for (const text of [
      '思考中',
      '思考了 57 秒',
      '思考了 57 秒思考了 57 秒',
      'Worked for 57s',
      'Thinking...',
    ]) {
      run('apiTextUpdateReasoning(writer,"status",text,false)', { text })
    }
    expect(run('writer.output')).toHaveLength(0)
    expect(events.some((event) => event.type === 'response.reasoning_summary_text.delta')).toBe(
      false
    )
    run('chatgptApiConsumeTextStreamEvent(state,writer,event)', {
      event: {
        message: message('actual', {
          content_type: 'thoughts',
          thoughts: [{ summary: '比较两个方案', content: '先核对用户给出的限制条件。' }],
        }),
      },
    })
    for (let i = 0; i < 2; i++)
      run('chatgptApiConsumeTextStreamEvent(state,writer,event)', {
        event: {
          message: message(
            'recap',
            { content_type: 'reasoning_recap', content: 'Worked for 57s' },
            { finished_duration_sec: 57 }
          ),
        },
      })
    const output = run('writer.output')
    expect(output).toHaveLength(1)
    expect(output[0]).toMatchObject({
      status: 'completed',
      summary: [{ text: '比较两个方案\n先核对用户给出的限制条件。' }],
    })
    expect(JSON.stringify(output)).not.toContain('57')
  })

  it('429 后指数退避并保存恢复时间，重启沿用等待时间', () => {
    const { run, context } = runtime()
    let clock = 0
    context.Date = class extends Date {
      static now() {
        return clock
      }
    }
    const polls = []
    const saved = []
    const originalCheckpoint = context.host.checkpoint
    context.host.checkpoint = (value) => {
      if (value) saved.push(JSON.parse(JSON.stringify(value)))
      return originalCheckpoint(value)
    }
    context.chatgptApiReceiveTextUpdates = () => {
      clock += 1000
    }
    context.chatgptApiTextConversationResponse = () => {
      polls.push(clock)
      if (polls.length <= 2) return { status: 429, ok: false }
      return {
        status: 200,
        ok: true,
        body: {
          messages: [
            message('user-new', { content_type: 'text', parts: ['问题'] }, {}, 'user'),
            message('answer', { content_type: 'text', parts: ['完成'] }),
          ],
        },
      }
    }
    expect(run('chatgptApiWaitForRichText({},"conversation","root",state,writer,{})')).toBe('完成')
    expect(polls).toEqual([30000, 90000, 210000])
    expect(saved.slice(0, 2).map((s) => s.poll_retry_ms)).toEqual([60000, 120000])
    expect(saved[1].poll_next_at_ms).toBe(210000)
    const restored = saved[1]
    clock = 0
    originalCheckpoint(restored)
    run('state=chatgptApiCreateTextStreamState()')
    expect(run('chatgptApiWaitForRichText({},"conversation","root",state,writer,{})')).toBe('完成')
    expect(polls.at(-1)).toBe(210000)
  })
  it('WebSocket 提前给出最终结果时不再请求会话快照', () => {
    const { run, context } = runtime()
    let clock = 0
    context.Date = class extends Date {
      static now() {
        return clock
      }
    }
    context.chatgptApiReceiveTextUpdates = (_ws, _id, consume) => {
      clock += 1000
      if (clock === 5000)
        consume({ message: message('answer', { content_type: 'text', parts: ['推送完成'] }) })
    }
    context.chatgptApiTextConversationResponse = () => {
      throw new Error('推送已完成不应再轮询')
    }
    expect(run('chatgptApiWaitForRichText({},"conversation","root",state,writer,{})')).toBe(
      '推送完成'
    )
    expect(clock).toBe(5000)
  })

  it('网页公开思考摘要进入独立 reasoning 流，隐藏分析及工具内部正文不泄漏', () => {
    const { run, events } = runtime()
    run('chatgptApiConsumeTextStreamEvent(state,writer,event)', {
      event: {
        p: '',
        o: 'add',
        v: {
          message: message('think', {
            content_type: 'thoughts',
            thoughts: [{ summary: '正在准备图片', content: '' }],
          }),
        },
      },
    })
    run('chatgptApiConsumeTextStreamEvent(state,writer,event)', {
      event: {
        message: message(
          'private',
          { content_type: 'text', parts: ['内部分析不可展示'] },
          { is_visually_hidden_from_conversation: true }
        ),
      },
    })
    expect(
      events
        .filter((e) => e.type === 'response.reasoning_summary_text.delta')
        .map((e) => e.delta)
        .join('')
    ).toBe('正在准备图片')
    expect(JSON.stringify(events)).not.toContain('内部分析不可展示')
  })
  it('图片回复没有正文也可完成，多个更新不会重复图片，不归档自由对话', () => {
    const { run, context, events } = runtime()
    const requests = []
    context.chatgptApiRequest = (_auth, method, path) => {
      requests.push({ method, path })
      return {
        ok: true,
        status: 200,
        body: { download_url: 'https://chatgpt.com/backend-api/estuary/content?id=test' },
      }
    }
    context.chatgptApiDownloadImageData = () => 'data:image/png;base64,aGVsbG8='
    const image = message(
      'image',
      {
        content_type: 'multimodal_text',
        parts: [
          {
            content_type: 'image_asset_pointer',
            asset_pointer: 'sediment://file_result',
            width: 1024,
            height: 1024,
          },
        ],
      },
      { ghostrider: { status: 'final' } },
      'tool'
    )
    run('chatgptApiConsumeTextStreamEvent(state,writer,event)', {
      event: { type: 'server_ste_metadata', metadata: { turn_use_case: 'image gen' } },
    })
    run('chatgptApiConsumeTextStreamEvent(state,writer,event)', { event: { message: image } })
    run('chatgptApiResolveTextImages({},"conversation",state,writer)')
    run('chatgptApiResolveTextImages({},"conversation",state,writer)')
    const response = run('apiTextCompleteResponse(writer)')
    expect(response.status).toBe('completed')
    expect(response.output_text).toBe('')
    expect(response.output.filter((i) => i.type === 'image_generation_call')).toHaveLength(1)
    expect(response.output[0]).toMatchObject({ result: 'aGVsbG8=', status: 'completed' })
    expect(requests).toEqual([
      {
        method: 'GET',
        path: '/backend-api/files/download/file_result?conversation_id=conversation&download_intent=false&inline=true&include_library_file_state=true',
      },
    ])
    expect(
      events.some(
        (e) => e.item?.type === 'image_generation_call' && e.item.status === 'in_progress'
      )
    ).toBe(true)
  })
  it('未完成产物、用户上传图及历史轮次不能当作新生图结果', () => {
    const { run, context } = runtime()
    context.chatgptApiRequest = () => {
      throw new Error('不应下载未完成图片')
    }
    const image = message(
      'partial',
      {
        content_type: 'multimodal_text',
        parts: [{ content_type: 'image_asset_pointer', asset_pointer: 'sediment://file_pending' }],
      },
      { ghostrider: { status: 'intermediate' } },
      'tool'
    )
    run('chatgptApiConsumeTextStreamEvent(state,writer,event)', { event: { message: image } })
    expect(run('chatgptApiResolveTextImages({},"conversation",state,writer)')).toBe(0)
    const user = message('user-new', { content_type: 'text', parts: ['新问题'] }, {}, 'user')
    expect(
      run('chatgptApiCurrentTurnMessages(conversation,"user-new","client-created-root")', {
        conversation: { messages: [image, user] },
      })
    ).toEqual([])
    expect(
      run('chatgptApiCurrentTurnMessages(conversation,"missing","client-created-root")', {
        conversation: { messages: [image, user] },
      })
    ).toEqual([])
  })
  it('多图结果等待全部可下载文件，不会因为第一张可用就提前完成', () => {
    const { run, context } = runtime()
    let ready = false
    const requests = []
    context.chatgptApiRequest = (_auth, _method, path) => {
      requests.push(path)
      return path.includes('file_second') && !ready
        ? { ok: false, status: 404 }
        : { ok: true, status: 200, body: { download_url: 'https://chatgpt.com/image.png' } }
    }
    context.chatgptApiDownloadImageData = () => 'data:image/png;base64,aGVsbG8='
    const image = message(
      'images',
      {
        content_type: 'multimodal_text',
        parts: ['first', 'second'].map((id) => ({
          content_type: 'image_asset_pointer',
          asset_pointer: 'sediment://file_' + id,
        })),
      },
      { ghostrider: { status: 'final' } },
      'tool'
    )
    run('chatgptApiConsumeTextStreamEvent(state,writer,event)', { event: { message: image } })
    expect(run('chatgptApiResolveTextImages({},"conversation",state,writer)')).toBe(1)
    expect(run('state.pending_image_files')).toBe(true)
    ready = true
    expect(run('chatgptApiResolveTextImages({},"conversation",state,writer)')).toBe(2)
    expect(run('state.pending_image_files')).toBe(false)
    expect(
      run(
        'apiTextCompleteResponse(writer).output.filter(item=>item.type==="image_generation_call").length'
      )
    ).toBe(2)
    expect(requests.filter((path) => path.includes('file_first'))).toHaveLength(1)
  })
  it('图片任务结束前的空 final 消息不会使对话提前结束', () => {
    const { run, context } = runtime()
    let reads = 0
    context.chatgptApiReceiveTextUpdates = () => {}
    context.chatgptApiTextConversationResponse = () => ({
      ok: true,
      status: 200,
      body: {
        messages: [
          message('user-new', { content_type: 'text', parts: ['生成图片'] }, {}, 'user'),
          message('blank', { content_type: 'text', parts: [''] }, {}, 'assistant'),
        ],
      },
    })
    context.chatgptApiResolveTextImages = () => (++reads >= 2 ? 1 : 0)
    run(
      'state.image_pending=true; chatgptApiWaitForRichText({},"conversation","client-created-root",state,writer,{})'
    )
    expect(reads).toBe(2)
  })
  it('消息连接限定当前会话，并保留网页 connect 与 pages 订阅', () => {
    const { run, context } = runtime()
    const calls = []
    context.chatgptApiRequest = () => ({
      ok: true,
      body: { websocket_url: 'wss://ws.chatgpt.com/p4/ws/user/test?verify=private' },
    })
    context.host.call = (action, params) => {
      calls.push({ action, params })
      if (action === 'http.websocket_open') return { connection_id: 'ws' }
      if (action === 'http.websocket_receive')
        return {
          text: JSON.stringify({
            type: 'conversation-update',
            payload: {
              conversation_id: 'other',
              update_content: { messages: [{ id: 'foreign' }] },
            },
          }),
        }
      return {}
    }
    run(
      'var connection=chatgptApiOpenTextUpdates({});chatgptApiReceiveTextUpdates(connection,"current",()=>{throw new Error("其他会话消息泄漏")});chatgptApiCloseTextUpdates(connection)'
    )
    expect(JSON.parse(calls.find((c) => c.action === 'http.websocket_send').params.text)).toEqual([
      { command: { presence: { state: 'foreground', type: 'presence' }, type: 'connect' }, id: 1 },
      { command: { topic_id: 'pages', type: 'subscribe' }, id: 2 },
    ])
  })
})

import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import { describe, expect, it } from 'vitest'

function runtime() {
  const events = []
  let saved = { user_message_id: 'current-user' }
  const ctx = vm.createContext({
    host: {
      checkpoint: (value) => {
        if (value) saved = value
        return saved
      },
      emit: (event) => events.push(JSON.parse(JSON.stringify(event))),
      call: () => ({ ok: true, body_base64: 'aGVsbG8=', headers: { 'content-type': 'image/png' } }),
      sleep: () => {},
    },
  })
  for (const file of [
    '_shared/api_session.js',
    '_shared/api_text_attachments.js',
    '_shared/api_text_responses.js',
    '_shared/api_text_media.js',
    'doubao-api/scripts/conversation.js',
    'doubao-api/scripts/text_media.js',
  ])
    vm.runInContext(readFileSync('../../plugins/' + file, 'utf8'), ctx)
  vm.runInContext(
    "var media=doubaoTextMediaState();var writer=apiTextCreateResponseWriter(apiTextNormalizeTextRequest({model:'doubao',input:'测试'}));var stream=doubaoStreamState()",
    ctx
  )
  return {
    ctx,
    events,
    run(code, values = {}) {
      Object.assign(ctx, values)
      return vm.runInContext(code, ctx)
    },
  }
}
const creationBlock = (creation) => ({
  block_id: 'media',
  block_type: 2074,
  content: { creation_block: { creations: [creation] } },
})

describe('豆包对话媒体协议', () => {
  it('普通文本请求不强制转换为独立生成接口或媒体模型', () => {
    const { run } = runtime()
    const body = run(
      "doubaoMessageId=()=> 'local-msg';doubaoLocalConversationId=()=> 'local-test';doubaoPayload('请生成一个5秒视频','text')"
    )
    expect(body.chat_ability).toBeUndefined()
    expect(body.option).toMatchObject({ agent_mode: 2, model_config: { model_item_key: '0' } })
    expect(body.messages[0].content_block[0].content.text_block.text).toBe('请生成一个5秒视频')
  })
  it('平台明确宣告异步视频生成后立即发出进度，不把用户提示词当作进度', () => {
    const { run, events } = runtime()
    run(
      "stream.text='请生成一个视频';doubaoTextMediaEvent({},media,writer,stream,{event:'SSE_REPLY_END',data:{end_type:3}})"
    )
    expect(events).toHaveLength(0)
    run(
      "stream.text='本次使用 Seedance 2.0 Mini 生成，大约需要1-3分钟。视频生成好后会发送。';doubaoTextMediaEvent({},media,writer,stream,{event:'STREAM_MSG_NOTIFY',data:{meta:{user_type:2,message_id:'reply'},content:{ext:{use_creation:'1'}}}})"
    )
    expect(events.at(-1).item).toMatchObject({
      type: 'video_generation_call',
      status: 'in_progress',
    })
    expect(run('host.checkpoint().media_kind')).toBe('video')
  })
  it('图片无正文也能完成，原图和视频共用 Responses 产物协议', () => {
    const { ctx, run } = runtime()
    ctx.doubaoVideoDownload = () => 'https://media.example.com/video.mp4?sign=private'
    run('doubaoCompleteTextMedia({},media,writer,"conversation",creations)', {
      creations: [
        {
          image: {
            key: 'image-1',
            status: 2,
            image_ori_raw: {
              url: 'https://image.example.com/original.png',
              width: 1024,
              height: 1024,
            },
          },
        },
        { video: { vid: 'video-1', status: 3, duration: 5 } },
      ],
    })
    const response = run('apiTextCompleteResponse(writer)')
    expect(response.status).toBe('completed')
    expect(response.output_text).toBe('')
    expect(response.output.map((item) => item.type)).toEqual([
      'image_generation_call',
      'video_generation_call',
    ])
    expect(response.output[1]).toMatchObject({
      duration: 5,
      provider_media_id: 'video-1',
      output_format: 'mp4',
    })
  })
  it('同时生成图片和视频时保留各自进度槽，完成后不残留生成中卡片', () => {
    const { ctx, run } = runtime()
    ctx.doubaoVideoDownload = () => 'https://media.example.com/ready.mp4'
    const image = {
      id: 'image-job',
      image: {
        key: 'image',
        status: 2,
        image_ori_raw: { url: 'https://image.example.com/raw.png' },
      },
    }
    const video = { id: 'video-job', video: { vid: 'video', status: 3, duration: 5 } }
    run('doubaoTextMediaBlocks(media,writer,blocks)', {
      blocks: { a: creationBlock(image), b: creationBlock(video) },
    })
    run('doubaoCompleteTextMedia({},media,writer,"conversation",creations)', {
      creations: [image, video],
    })
    const output = run('apiTextCompleteResponse(writer).output')
    expect(output).toHaveLength(2)
    expect(output.every((item) => item.status === 'completed')).toBe(true)
    expect(output[1].provider_media_id).toBe('video')
  })

  it('回读只接收当前用户消息之后的产物，恢复时可用已保存的本地消息 ID 定位', () => {
    const { run } = runtime()
    const messages = [
      { message_id: 'old', user_type: 2, index_in_conv: 2 },
      {
        message_id: 'current-user',
        local_message_id: 'local-current',
        user_type: 1,
        index_in_conv: 6,
      },
      { message_id: 'new', bot_reply_message_id: 'current-user', user_type: 2, index_in_conv: 8 },
    ]
    expect(
      run('doubaoTextTurnMessages(messages,media)', { messages }).map((m) => m.message_id)
    ).toEqual(['new'])
    run("host.checkpoint({local_message_id:'local-current'});media.user_message_id=''")
    expect(
      run('doubaoTextTurnMessages(messages,media)', { messages }).map((m) => m.message_id)
    ).toEqual(['new'])
    run('host.checkpoint({});media.user_message_id=""')
    expect(run('doubaoTextTurnMessages(messages,media)', { messages })).toEqual([])
  })
  it('只读恢复异步视频，不能把通知正文结束当成媒体任务完成', () => {
    const { ctx, run, events } = runtime()
    let polls = 0
    ctx.doubaoHistory = () =>
      ++polls === 1
        ? [
            { message_id: 'current-user', user_type: 1, index_in_conv: 1 },
            {
              message_id: 'reply',
              bot_reply_message_id: 'current-user',
              user_type: 2,
              index_in_conv: 2,
              ext: { is_finish: '1', use_creation: '1' },
              content_block: [
                {
                  block_id: 'text',
                  block_type: 10000,
                  content: { text_block: { text: '视频正在生成，大约需要1-3分钟。' } },
                },
              ],
            },
          ]
        : [
            { message_id: 'current-user', user_type: 1, index_in_conv: 1 },
            {
              message_id: 'video',
              bot_reply_message_id: 'current-user',
              user_type: 2,
              index_in_conv: 3,
              ext: { is_finish: '1' },
              content_block: [
                creationBlock({ video: { vid: 'ready-video', status: 3, duration: 5 } }),
              ],
            },
          ]
    ctx.doubaoCheckCommerce = () => {}
    ctx.doubaoHeartbeatTick = () => {}
    ctx.doubaoConfirmMediaIfRequested = () => false
    ctx.doubaoVideoDownload = () => 'https://media.example.com/ready.mp4'
    run('doubaoWaitForTextMedia({},media,writer,"conversation")')
    expect(polls).toBe(2)
    expect(
      events.some(
        (event) => event.item?.type === 'video_generation_call' && event.item.status === 'completed'
      )
    ).toBe(true)
  })
  it('图片拒绝和视频失败都中断等待，不能假装完成', () => {
    const { ctx, run } = runtime()
    ctx.doubaoCheckCommerce = () => {}
    for (const creation of [
      { image: { key: 'bad-image', status: 3 } },
      { video: { vid: 'bad-video', status: 4 } },
    ]) {
      ctx.doubaoHistory = () => [
        { message_id: 'current-user', user_type: 1, index_in_conv: 1 },
        {
          message_id: 'reply',
          bot_reply_message_id: 'current-user',
          user_type: 2,
          index_in_conv: 2,
          content_block: [creationBlock(creation)],
        },
      ]
      expect(() => run('doubaoWaitForTextMedia({},media,writer,"conversation")')).toThrow(
        creation.image ? 'content_policy_refusal' : 'platform_rejected'
      )
    }
  })
})

import { describe, expect, it } from 'vitest'
import { scripts } from './platform-api-test-helper.mjs'

describe('豆包媒体参数', () => {
  const run = scripts(['doubao-api/scripts/media_options.js'])
  run(
    `apiTextUuid=()=> 'test-id'; doubaoConversationActions=(session,id)=>{
    session.actionBar={conf:{item_list:configuration}};
    return [{item_id:'image',instruction_type:4},{item_id:'video',instruction_type:18}]
  }`,
    {
      configuration: Object.fromEntries(
        [
          ['image', 'model', 'Seedream 5.0 Pro', 'image_creation_ratio', ['auto', '3:2']],
          ['video', 'video-model', 'seedance_v2.0_mini', 'video-ratio', ['auto', '16:9']],
        ].map(([kind, key, model, ratioKey, ratios]) => [
          kind,
          {
            instruction_conf: {
              instruction_item_list: [
                {
                  template: JSON.stringify({
                    selector_list: [
                      {
                        template: JSON.stringify({
                          key,
                          option_list: [{ id: 1, option_key: model }],
                        }),
                      },
                      {
                        template: JSON.stringify({
                          configs: {
                            1: {
                              id: model,
                              selectors: [
                                {
                                  template: JSON.stringify({
                                    key: ratioKey,
                                    default_selection_id: 0,
                                    option_list: ratios.map((ratio, id) => ({
                                      id,
                                      option_key: ratio,
                                    })),
                                  }),
                                },
                                {
                                  template: JSON.stringify({
                                    key: 'video-duration',
                                    lower_bound: 4,
                                    upper_bound: 15,
                                    step_length: 1,
                                    default_selected_duration: 10,
                                  }),
                                },
                              ],
                            },
                          },
                        }),
                      },
                    ],
                  }),
                },
              ],
            },
          },
        ])
      ),
    }
  )
  it('仅接受具体 model ID，不接受默认入口或缺失模型', () => {
    expect(() => run(`doubaoMediaOptions({model_id:'doubao-image'}, {}, 'image')`)).toThrow(
      '必须指定具体'
    )
    expect(() => run(`doubaoMediaOptions({}, {}, 'image')`)).toThrow('必须指定具体')
    expect(
      run(
        `doubaoMediaOptions({model_id:'doubao-seedream-5-pro',image_aspect_ratio:'3:2'}, {}, 'image')`
      )
    ).toMatchObject({ model: 'Seedream 5.0 Pro', ratio: '3:2' })
  })
  it.each([4, 7, 10, 15])('接受平台允许的 %s 秒视频', (seconds) => {
    expect(
      run(`doubaoMediaOptions({model_id:'doubao-seedance-2-mini',seconds}, {}, 'video')`, {
        seconds,
      }).duration
    ).toBe(seconds)
  })
  it.each([0, 3, 16, 4.5, 'bad'])('拒绝平台不支持的秒数 %s', (seconds) => {
    expect(() =>
      run(`doubaoMediaOptions({model_id:'doubao-seedance-2-mini',seconds}, {}, 'video')`, {
        seconds,
      })
    ).toThrow('invalid_input')
  })
  it('拒绝模型和参数冲突以及不支持的比例', () => {
    expect(() =>
      run(
        `doubaoMediaOptions({model_id:'doubao-seedream-5-pro',generation_options:{model:'other'}}, {}, 'image')`
      )
    ).toThrow('不一致')
    expect(() =>
      run(
        `doubaoMediaOptions({model_id:'doubao-seedance-2-mini',video_aspect_ratio:'21:9'}, {}, 'video')`
      )
    ).toThrow('不支持该比例')
  })
  it('恢复任务仍要求相同的具体模型，不能用旧 checkpoint 绕过模型校验', () => {
    const saved = { model: 'Seedream 5.0 Pro', ratio: '3:2', conversationId: 'local_saved' }
    expect(
      run(`doubaoMediaOptions({model_id:'doubao-seedream-5-pro'}, {}, 'image', saved)`, { saved })
    ).toEqual(saved)
    for (const model_id of [undefined, 'doubao-image', 'doubao-seedream-5-flash']) {
      expect(() =>
        run(`doubaoMediaOptions({model_id}, {}, 'image', saved)`, { model_id, saved })
      ).toThrow('invalid_input')
    }
  })
})

describe('豆包原会话自动确认', () => {
  it.each(['请充值后确认生成', '请确认授权并接受使用规范后生成', '请输入验证码，确认后开始生成'])(
    '额外授权确认不作为普通参数确认自动发送：%s',
    (reply) => {
      const run = scripts(['doubao-api/scripts/history.js'])
      expect(() =>
        run(
          `doubaoConfirmMediaIfRequested({}, '123', 'video', [{
        user_type:2,message_id:'789',index_in_conv:5,ext:{is_finish:'1'},
        content_block:[{content:{text_block:{text:reply}}}]
      }])`,
          { reply }
        )
      ).toThrow('generation_requires_confirmation')
    }
  )

  it('消耗账户现有付费额度的生成确认仍自动在原会话继续', () => {
    let checkpoint = {}
    const submitted = []
    const run = scripts(['doubao-api/scripts/conversation.js', 'doubao-api/scripts/history.js'], {
      checkpoint: (value) => (value ? (checkpoint = value) : checkpoint),
    })
    run(
      `apiTextUuid=()=> 'confirmation';doubaoImRequest=()=>({downlink_body:{get_conv_info_downlink_body:{
      conversation_info:{conversation_id:'123',last_section_id:'456',latest_index:8}
    }}});doubaoSubmit=submit;`,
      { submit: (_session, payload) => submitted.push(payload) }
    )
    run(`doubaoConfirmMediaIfRequested({},'123','video',[{
      user_type:2,message_id:'paid-confirm',index_in_conv:8,ext:{is_finish:'1'},
      content_block:[{content:{text_block:{text:'今日视频生成免费额度已用完，本次将消耗付费额度。是否继续生成？确认生成'}}}]
    }])`)
    expect(submitted).toHaveLength(1)
    expect(submitted[0].client_meta.conversation_id).toBe('123')
    expect(submitted[0].option.need_create_conversation).toBe(false)
    expect(checkpoint.confirmation_replies[0].status).toBe('sent')
  })

  it('保留原会话和最新消息位置，最多发送一次相同确认', () => {
    let checkpoint = {}
    const submitted = []
    const run = scripts(['doubao-api/scripts/conversation.js', 'doubao-api/scripts/history.js'], {
      checkpoint: (value) => (value ? (checkpoint = value) : checkpoint),
    })
    run(
      `apiTextUuid=()=> 'new-id'; doubaoImRequest=()=>({downlink_body:{get_conv_info_downlink_body:{
      conversation_info:{conversation_id:'123',last_section_id:'456',latest_index:5}
    }}});doubaoSubmit=submit;`,
      { submit: (_session, payload) => submitted.push(payload) }
    )
    const messages = [
      {
        user_type: 2,
        message_id: '789',
        index_in_conv: '5',
        ext: { is_finish: '1' },
        content_block: [{ content: { text_block: { text: '确认后我再开始生成视频。' } } }],
      },
    ]
    run(`doubaoConfirmMediaIfRequested({},'123','video',messages)`, { messages })
    run(`doubaoConfirmMediaIfRequested({},'123','video',messages)`)
    expect(submitted).toHaveLength(1)
    const payload = submitted[0]
    expect(payload.client_meta).toMatchObject({
      conversation_id: '123',
      last_section_id: '456',
      last_message_index: 5,
    })
    expect(payload.client_meta.local_conversation_id).toBeUndefined()
    expect(payload.option.need_create_conversation).toBe(false)
    expect(payload.option.conversation_init_option).toBeUndefined()
    expect(payload.messages[0].content_block[0].content.text_block.text).toBe(
      '确认，按上述参数直接生成视频。'
    )
    expect(checkpoint.confirmation_replies[0].status).toBe('sent')
    checkpoint.confirmation_replies[0].status = 'submitting'
    expect(() => run(`doubaoConfirmMediaIfRequested({},'123','video',messages)`)).toThrow(
      'PROVIDER_SUBMISSION_UNKNOWN'
    )
  })

  it('文字先完成时仍等待后续视频卡片，不在第六次回读时误报失败', () => {
    let reads = 0
    const run = scripts(['doubao-api/scripts/history.js'], { sleep: () => {} })
    run(`doubaoHistory=read;doubaoConfirmMediaIfRequested=()=>{}`, {
      read: () => {
        reads++
        return [
          {
            user_type: 2,
            ext: { is_finish: '1' },
            content_block:
              reads < 8
                ? []
                : [
                    {
                      content: {
                        creation_block: { creations: [{ video: { vid: 'result', status: 3 } }] },
                      },
                    },
                  ],
          },
        ]
      },
    })
    expect(run(`doubaoWaitForResult({},'123','video')`)[0].video.vid).toBe('result')
    expect(reads).toBe(8)
  })

  it('平台已明确排队时等待超过五分钟的生成，不把排队当作无结果', () => {
    let elapsed = 0
    const run = scripts(['doubao-api/scripts/history.js'], {
      sleep: () => {
        elapsed += 60_000
      },
    })
    run(
      `Date.now=clock;doubaoConfirmMediaIfRequested=()=>{};doubaoHistory=()=>[{
      user_type:2,ext:{is_finish:'1'},content_block:clock()<8*60_000
        ? [{content:{text_block:{text:'预计等待 5 分钟，生成好后会主动发送给你。'}}}]
        : [{content:{creation_block:{creations:[{video:{vid:'finished',status:3}}]}}}]
    }]`,
      { clock: () => elapsed }
    )
    expect(run(`doubaoWaitForResult({},'123','video')`)[0].video.vid).toBe('finished')
    expect(elapsed).toBe(8 * 60_000)
  })
})

describe('豆包参考视频上传', () => {
  it('按网页 VOD 协议准备、分片校验、完成并提交，保留真实视频身份', () => {
    const calls = []
    const run = scripts(['doubao-api/scripts/video_upload.js'], {
      call: (action, input) => {
        calls.push([action, input])
        if (action === 'file.inspect_media') return { has_video: true, duration: 5.05 }
        if (action === 'file.read_base64') return { base64: 'bytes', size: 15 * 1024 * 1024 - 100 }
        if (action === 'crypto.digest_bytes')
          return { value: input.algorithm === 'md5' ? 'md5' : 'crc' }
        throw new Error(action)
      },
    })
    run(
      `apiTextUuid=()=> 'unique';doubaoHex=value=>value;
      doubaoRequest=()=>({service_id:'space',upload_auth_token:{}});
      doubaoImagex=(_session,_auth,params,body,service)=>{
        record(['signed',params,body,service]);
        return params.Action==='ApplyUploadInner'
          ? {InnerUploadAddress:{UploadNodes:[{UploadHost:'upload.example.com',StoreInfos:[{StoreUri:'path',Auth:'auth'}],SessionKey:'session'}]}}
          : {Results:[{Vid:'video-id',VideoMeta:{Width:960,Height:960}}]};
      };
      doubaoVideoPart=(_session,_store,url,body,_type,crc)=>{
        record(['part',url,body]); return url.includes('phase=init')?{uploadid:'upload-id'}:{crc32:crc};
      }`,
      { record: (value) => calls.push(value) }
    )
    const result = run(`doubaoUploadVideoReference({}, {}, '/test/video.mp4','message',{
      referenceMedia:{max_ref_video_count:1,max_ref_video_duration:15}
    })`)
    expect(result).toMatchObject({
      vid: 'video-id',
      uri: 'vod://space/video-id',
      width: 960,
      height: 960,
      duration: 5.05,
    })
    expect(
      calls
        .filter(([action, value]) => action === 'file.read_base64' && value.length)
        .map(([, value]) => [value.offset, value.length])
    ).toEqual([
      [0, 5 * 1024 * 1024],
      [5 * 1024 * 1024, 10 * 1024 * 1024 - 100],
    ])
    expect(
      calls
        .filter(([action]) => action === 'signed')
        .map(([, params, , service]) => [params.Action, service])
    ).toEqual([
      ['ApplyUploadInner', 'vod'],
      ['CommitUploadInner', 'vod'],
    ])
    expect(calls.filter(([action]) => action === 'part').at(-1)[2]).toBe('1:crc,2:crc')
    expect(() => run(`doubaoUploadVideoReference({}, {}, '/test/video.mp4','message',{})`)).toThrow(
      '不支持参考视频'
    )
  })
})

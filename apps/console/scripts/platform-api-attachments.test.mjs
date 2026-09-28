import { describe, expect, it } from 'vitest'
import { scripts } from './platform-api-test-helper.mjs'

const shared = ['_shared/api_text_attachments.js', '_shared/api_text_responses.js']

describe('API 插件共同附件协议', () => {
  it('自定义后缀保留文件名，使用宿主提供的 MIME 类型', () => {
    const run = scripts(shared)
    expect(
      run(`apiTextResponsesMedia([{type:'input_file',filename:'示例.c++',
      mime_type:'text/plain',file_data:'YWJj'}])`)[0]
    ).toMatchObject({
      name: '示例.c++',
      mime_type: 'text/plain',
      size: 3,
    })
  })

  it('保留图片、办公文档和音频的原始内容与文件身份', () => {
    const run = scripts(shared)
    const result = run('apiTextNormalizeTextRequest(request)', {
      request: {
        input: [
          {
            role: 'user',
            content: [
              { type: 'input_text', text: '核对附件' },
              { type: 'input_image', image_url: 'data:image/png;base64,YWJj' },
              { type: 'input_file', filename: '核验报告.docx', file_data: 'YWJj' },
              { type: 'input_audio', input_audio: { format: 'mp3', data: 'YWJj' } },
            ],
          },
        ],
      },
    })
    expect(result.input[0].text).toBe('核对附件')
    expect(result.input[0].attachments.map((file) => file.mime_type)).toEqual([
      'image/png',
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
      'audio/mpeg',
    ])
    expect(
      result.input[0].attachments.every((file) => file.base64 === 'YWJj' && file.size === 3)
    ).toBe(true)
  })

  it.each([
    { type: 'input_file', filename: '../credential.txt', file_data: 'YWJj' },
    { type: 'input_file', filename: 'a.txt', file_data: 'YWJj=' },
    { type: 'input_image', image_url: 'data:text/plain;base64,YWJj' },
    { type: 'input_file', filename: 'a.txt', file_id: 'foreign-file' },
  ])('在上传前拒绝无效附件 %#', (part) => {
    const run = scripts(shared)
    expect(() => run('apiTextResponsesMedia([part])', { part })).toThrow('invalid_input')
  })

  it('文件原子写入回执不完整时不继续上传', () => {
    const run = scripts(shared, { call: () => ({ size: 2 }) })
    expect(() =>
      run(`apiTextMaterializeAttachment({workspace_dir:'/tmp/job'},
      {name:'a.txt',size:3,base64:'YWJj'},0)`)
    ).toThrow('落盘不完整')
  })
})

describe('豆包网页附件与生成协议', () => {
  it('视频比例同时进入正文和能力参数，附件预处理与发送共用消息身份', () => {
    const run = scripts([...shared, 'doubao-api/scripts/conversation.js'])
    const payload = run(`doubaoPayload('杯子', 'video', {
      model:'seedance_v2.0_mini',ratio:'1:1',duration:5,references:[{block_type:10052}],reference_message_id:'attachment-message'
    })`)
    expect(payload.messages[0].local_message_id).toBe('attachment-message')
    expect(payload.messages[1].content_block[0].content.text_block.text).toBe(
      '生成视频：杯子，1:1，5s'
    )
    expect(JSON.parse(payload.chat_ability.ability_param)).toMatchObject({
      ratio: '1:1',
      duration: 5,
    })
    expect(payload.option).not.toHaveProperty('general_task_param')
    expect(payload.ext.sub_conv_firstmet_type).toBe('1')
    expect(payload.ext.collection_id).not.toBe('')
    expect(payload.option.collect_id).toBe(payload.ext.collection_id)
  })

  it('音频按资源类型3登记，消息作为文件类型1引用真实时长', () => {
    const calls = []
    const run = scripts([...shared, 'doubao-api/scripts/upload.js'])
    run(`doubaoUploadBinary=()=> 'resource/audio.mp3'; doubaoRequest=request`, {
      request: (_session, path, body) => {
        calls.push({ path, body })
        return { 'resource/audio.mp3': { resource_id: 'resource-1' } }
      },
    })
    const file = run(`doubaoPrepareAttachment({}, {
      name:'test.mp3',mime_type:'audio/mpeg',size:10,duration:1.8,
      selected_at:123,audio_attachment_id:'audio-1'
    }, 'message-1')`)
    expect(calls).toHaveLength(1)
    expect(calls[0].path).toBe('/alice/resource/register')
    expect(calls[0].body.resource_list[0]).toMatchObject({
      resource_type: 3,
      meta: { audio_meta: { parse_extra: { local_message_id: 'message-1' } } },
    })
    const block = run('doubaoReferenceBlocks([file])', { file })[0]
    expect(block.content.attachment_block.attachments[0]).toMatchObject({
      type: 3,
      file: { file_type: 1, duration: 1.8 },
      extra: { extra: { audio_attachment_id: 'audio-1', audio_duration_ms: '1800' } },
    })
  })

  it('不接受网页当前不支持的视频附件或视频生成音频参考', () => {
    const run = scripts([...shared, 'doubao-api/scripts/upload.js'])
    expect(() =>
      run(`doubaoUploadTextAttachments({}, {}, {input:[{attachments:[{mime_type:'video/mp4'}]}]})`)
    ).toThrow('不支持视频文件附件')
    expect(() => run(`doubaoUploadReferences({ref_audio_locals:['audio.mp3']}, {})`)).toThrow(
      '不支持音频参考'
    )
  })

  it('混合附件中最后一个不受支持时，不先上传前面的文件', () => {
    const calls = []
    const run = scripts([...shared, 'doubao-api/scripts/upload.js'], {
      call: (...args) => calls.push(args),
    })
    expect(() =>
      run(`doubaoUploadTextAttachments({}, {}, {input:[{attachments:[
        {name:'a.txt',mime_type:'text/plain',base64:'YWJj',size:3},
        {name:'b.mp4',mime_type:'video/mp4',base64:'YWJj',size:3}
      ]}]})`)
    ).toThrow('不支持视频文件附件')
    expect(calls).toEqual([])
  })
})

describe('DeepSeek 多附件解析', () => {
  it('按网页顺序先解析附件和取得 PoW，再创建会话，并持久保存上传身份', () => {
    const calls = []
    const checkpoints = []
    const run = scripts(
      [...shared, 'deepseek-api/scripts/attachments.js', 'deepseek-api/scripts/generate_text.js'],
      {
        checkpoint: (value) => {
          if (value) checkpoints.push(value)
          return {}
        },
      }
    )
    run(
      `apiSessionRead=()=>({}); deepseekUploadAttachments=upload;
      deepseekPow=pow; deepseekRequest=create`,
      {
        upload: () => {
          calls.push('upload')
          return ['file-1']
        },
        pow: () => {
          calls.push('pow')
          return 'proof'
        },
        create: (_session, path) => {
          calls.push(path)
          throw new Error('stop after create')
        },
      }
    )
    expect(() => run(`main({request:{input:'读取附件'}})`)).toThrow('stop after create')
    expect(calls).toEqual(['upload', 'pow', '/chat_session/create'])
    expect(checkpoints[0]).toMatchObject({ phase: 'ready', attachment_file_ids: ['file-1'] })
  })

  it('创建会话之前验证全部附件，不为不支持的混合附件创建空会话', () => {
    const calls = []
    const run = scripts(
      [...shared, 'deepseek-api/scripts/attachments.js', 'deepseek-api/scripts/generate_text.js'],
      { checkpoint: () => ({}), call: (...args) => calls.push(args) }
    )
    run('apiSessionRead=read; deepseekRequest=read', {
      read: (...args) => calls.push(args),
    })
    expect(() =>
      run(`main({request:{input:[{role:'user',content:[
        {type:'input_text',text:'读取附件'},
        {type:'input_file',filename:'a.txt',file_data:'YWJj'},
        {type:'input_file',filename:'b.mp3',file_data:'YWJj'}
      ]}]}})`)
    ).toThrow('不支持音视频附件')
    expect(calls).toEqual([])
  })

  it('使用重复 file_ids 查询，所有附件解析成功才返回', () => {
    const calls = []
    let uploaded = 0
    const run = scripts([...shared, 'deepseek-api/scripts/attachments.js'], {
      call: (action, params) => {
        if (action === 'file.write_base64') return { size: 3 }
        if (action === 'http.request') {
          expect(params.body_format).toBe('multipart_file')
          expect(params.body_fields).toBeUndefined()
          return {
            ok: true,
            body: { code: 0, data: { biz_code: 0, biz_data: { id: 'file-' + ++uploaded } } },
          }
        }
        throw new Error('意外的 Host 操作')
      },
      sleep: () => {},
    })
    run(
      `const DEEPSEEK_ORIGIN='https://chat.deepseek.com';
      deepseekPow=()=> 'challenge'; deepseekHeaders=()=> ({}); deepseekRefreshSession=()=>{};
      apiSessionRequireOk=(result)=>result; deepseekRequest=request`,
      {
        request: (_session, path, _body, method) => {
          calls.push({ path, method })
          return {
            files: [
              { id: 'file-1', status: 'SUCCESS' },
              { id: 'file-2', status: 'SUCCESS' },
            ],
          }
        },
      }
    )
    const ids = run(`deepseekUploadAttachments({workspace_dir:'/tmp/job'}, {}, {
      reasoning:{effort:'none'}, input:[{attachments:[
        {name:'a.txt',mime_type:'text/plain',base64:'YWJj',size:3},
        {name:'b.pdf',mime_type:'application/pdf',base64:'YWJj',size:3}
      ]}]
    })`)
    expect(ids).toEqual(['file-1', 'file-2'])
    expect(calls).toEqual([
      { path: '/file/fetch_files?file_ids=file-1&file_ids=file-2', method: 'GET' },
    ])
  })
})

describe('DeepSeek 断流恢复', () => {
  it('合并增量与完整消息，只输出新增正文，不泄露思考片段', () => {
    const parts = []
    const run = scripts(['_shared/api_session.js', 'deepseek-api/scripts/stream.js'])
    run('apiTextFeedResponseText=feed; state=deepseekStreamState()', {
      feed: (_writer, text) => {
        if (text) parts.push(text)
      },
    })
    run(`deepseekConsume(state, {}, {event:'message',data:{v:{response:{status:'WIP',fragments:[
      {type:'THINK',content:'private reasoning'}, {type:'RESPONSE',content:'星河'}
    ]}}}},()=>{})`)
    run(
      `deepseekConsume(state, {}, {event:'message',data:{p:'response/fragments/-1/content',o:'APPEND',v:'杯子'}},()=>{})`
    )
    run(`deepseekConsume(state, {}, {code:0,data:{biz_code:22,biz_data:{message:{
      message_id:2,status:'FINISHED',fragments:[{type:'THINK',content:'private reasoning'},
        {type:'RESPONSE',content:'星河杯子73921'}]
    }}}},()=>{})`)
    expect(parts).toEqual(['星河', '杯子', '73921'])
    expect(run('state.finished')).toBe(true)
  })

  it('服务端未完成时不报成功，改写已发送的正文时明确失败', () => {
    const run = scripts(['deepseek-api/scripts/stream.js'])
    run('apiTextFeedResponseText=()=>{}; state=deepseekStreamState()')
    run(
      `deepseekSnapshot(state, {}, {status:'INCOMPLETE',fragments:[{type:'RESPONSE',content:'未完'}]})`
    )
    expect(run('state.finished')).toBe(false)
    expect(run('state.status')).toBe('INCOMPLETE')
    expect(() =>
      run(
        `deepseekSnapshot(state, {}, {status:'FINISHED',fragments:[{type:'RESPONSE',content:'另一段'}]})`
      )
    ).toThrow('已输出文本被改写')
  })
})

describe('ChatGPT 音视频附件完成回执', () => {
  it('没有索引阶段时使用处理完成事件中的资料库身份', () => {
    const run = scripts(['chatgpt/scripts/reference_upload.js'])
    const bodies = []
    run('chatgptApiRequireOk=(result)=>result; chatgptApiRequest=request', {
      request: (_auth, _method, _path, options) => {
        bodies.push(options.body)
        return {
          ok: true,
          body_text: JSON.stringify({
            event: 'file.processing.completed',
            extra: {
              metadata_object_id: 'libfile-audio',
              library_file_name: 'test.mp3',
              mime_type: 'audio/mpeg',
            },
          }),
        }
      },
    })
    const result = run(`chatgptApiProcessReference({},'file-audio',{
      name:'test.mp3',mime_type:'audio/mpeg',use_case:'ace_upload'
    })`)
    expect(result.library_file_id).toBe('libfile-audio')
    expect(bodies[0].mime_type).toBe('audio/mpeg')
  })
})

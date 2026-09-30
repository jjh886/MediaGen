import { describe, expect, it } from 'vitest'
import { scripts } from './platform-api-test-helper.mjs'

describe('豆包独立签名的指纹传递', () => {
  it('流式响应先交付响应头，clone 的正文读取等真实流结束后再完成', async () => {
    const run = scripts(['doubao-api/scripts/signing_transport.js'])
    run(`transport=doubaoSecurityTransport({drain_jobs:()=>{}},class Headers{});
      transport.prepare('https://example.com/stream');
      headerStatus=0; bodyText=null;
      transport.fetch('https://example.com/stream').then(response=>{
        headerStatus=response.status;
        response.clone().text().then(text=>{bodyText=text});
      });
      transport.signed();
      transport.complete({status:200,ok:true,headers:{}});`)
    await Promise.resolve()
    expect(run('headerStatus')).toBe(200)
    expect(run('bodyText')).toBeNull()
    run("transport.finish({body_text:'data: 完整正文\\n\\n'})")
    await Promise.resolve()
    expect(run('bodyText')).toBe('data: 完整正文\n\n')
  })

  it('把辅助 HTTP 的真实状态、响应头和正文交回 SDK，后续业务请求沿用同一上下文', async () => {
    const calls = []
    const run = scripts([
      'doubao-api/scripts/signing_transport.js',
      'doubao-api/scripts/signing_environment.js',
    ])
    const runtime = run('doubaoSecurityRuntime(input)', {
      input: {
        fingerprint: { user_agent: 'test-agent', plugins: [] },
        cookie: '',
        storage: {},
        started_at: Date.now(),
        url: 'https://www.doubao.com/chat/',
        config: { paths: { include: [], exclude: [] } },
        drain_jobs: () => {},
        send: (request) => {
          calls.push(request)
          return {
            status: 503,
            ok: false,
            headers: { 'x-ms-token': 'server-token' },
            body_text: '{"retry":true}',
          }
        },
        sdk: `globalThis.__LOADABLE_LOADED_CHUNKS__=[[0,{ISb:function(){
          globalThis.bdms={init:function(){
            localStorage.setItem('identity','persistent');
            sessionStorage.setItem('identity','tab');
            sessionStorage.named='value';
            localStorage.setItem('storageCheck',JSON.stringify({
              local:localStorage.getItem('identity'),session:sessionStorage.identity,
              keys:Object.keys(sessionStorage),length:sessionStorage.length,
              first:sessionStorage.key(0),missing:sessionStorage.getItem('missing')
            }));
            const xhr=new XMLHttpRequest();
            xhr.open('POST','https://mssdk.bytedance.com/web/r/token');
            xhr.setRequestHeader('Content-Type','application/json');
            xhr.onload=function(){ localStorage.setItem('reply',JSON.stringify({
              status:xhr.status,body:xhr.responseText,token:xhr.getResponseHeader('x-ms-token')
            })) };
            xhr.send('{"request":true}');
            const original=fetch;
            globalThis.fetch=(...args)=>original(...args).then(response=>{
              localStorage.setItem('businessStatus',String(response.status));return response;
            });
          }};
        }}]]`,
      },
    })
    await Promise.resolve()
    expect(JSON.parse(runtime.storage.storageCheck)).toEqual({
      local: 'persistent',
      session: 'tab',
      keys: ['identity', 'named'],
      length: 2,
      first: 'identity',
      missing: null,
    })
    expect(runtime.session_storage.named).toBe('value')
    expect(runtime.storage.named).toBeUndefined()
    expect(calls).toHaveLength(1)
    expect(calls[0].body).toBe('{"request":true}')
    expect(JSON.parse(runtime.storage.reply)).toEqual({
      status: 503,
      body: '{"retry":true}',
      token: 'server-token',
    })
    const request = {
      url: 'https://www.doubao.com/test?a=1',
      method: 'POST',
      headers: {},
      body: '{"message":1}',
    }
    expect(runtime.sign(request)).toEqual(request)
    expect(calls).toHaveLength(1)
    runtime.complete({ status: 201, ok: true, headers: {}, body_text: '{}' })
    await Promise.resolve()
    expect(runtime.storage.businessStatus).toBe('201')
    expect(runtime.sign({ ...request, body: '{"message":2}' }).body).toBe('{"message":2}')
    expect(calls).toHaveLength(1)
  })

  it('按真实到期时间运行或取消定时任务，不提前执行 SDK 的延迟请求', () => {
    let now = 1000
    const run = scripts(['doubao-api/scripts/signing_transport.js'])
    run(
      `clock=doubaoSecurityTransport({drain_jobs:()=>{}},class Headers{});
      executed=0; cancelled=clock.setTimeout(()=>{executed+=100},100);
      clock.clearTimeout(cancelled);clock.setTimeout(()=>{executed++},100);
      repeated=clock.setInterval(()=>{executed+=10},200)`,
      { Date: { now: () => now } }
    )
    run('clock.tick()')
    expect(run('executed')).toBe(0)
    now = 1100
    run('clock.tick()')
    expect(run('executed')).toBe(1)
    now = 1200
    run('clock.tick(); clock.clearInterval(repeated)')
    now = 1600
    run('clock.tick()')
    expect(run('executed')).toBe(11)
  })

  it('保留真实插件信息和 webdriver 值，支持 SDK 的具名属性访问', () => {
    const run = scripts([
      'doubao-api/scripts/signing_transport.js',
      'doubao-api/scripts/signing_environment.js',
    ])
    const result = run('doubaoSecurityRuntime(input)', {
      input: {
        drain_jobs: () => {},
        fingerprint: {
          user_agent: 'test-agent',
          webdriver: true,
          plugins: [
            {
              name: 'PDF Viewer',
              filename: 'internal-pdf-viewer',
              description: 'PDF',
              mime_types: [{ type: 'application/pdf', suffixes: 'pdf', description: 'PDF' }],
            },
          ],
        },
        storage: {},
        cookie: '',
        url: 'https://www.doubao.com/test?aid=1',
        method: 'GET',
        started_at: Date.now(),
        config: { paths: { include: [], exclude: [] } },
        sdk: `globalThis.__LOADABLE_LOADED_CHUNKS__=[[0,{ISb:function(){
          globalThis.bdms={init:function(){
            const plugin=navigator.plugins.namedItem('PDF Viewer');
            const mime=plugin.namedItem('application/pdf');
            localStorage.setItem('observed',JSON.stringify({
              webdriver:navigator.webdriver,filename:plugin.filename,
              named:navigator.plugins['PDF Viewer']===plugin,
              indexed:navigator.plugins.item(0)===plugin,
              mimeType:plugin['application/pdf'].type,
              linked:mime.enabledPlugin===plugin
            }));
          }};
        }}]]`,
      },
    })
    expect(JSON.parse(result.storage.observed)).toEqual({
      webdriver: true,
      filename: 'internal-pdf-viewer',
      named: true,
      indexed: true,
      mimeType: 'application/pdf',
      linked: true,
    })
  })
})

describe('API 会话凭据刷新边界', () => {
  it('已登录页面未再次请求账号信息时，从当前网页公共参数构建账号检查', () => {
    const run = scripts(['doubao-api/scripts/config.js'])
    const common = new URL('https://www.doubao.com/samantha/user/ab/get')
    for (const key of [
      'pc_version',
      'version_code',
      'language',
      'device_platform',
      'aid',
      'real_aid',
      'pkg_type',
      'use-olympus-account',
      'region',
      'sys_region',
      'web_id',
    ])
      common.searchParams.set(key, 'observed-' + key)
    const account = run('doubaoBrowserAccountUrl(resources).href', {
      URL,
      resources: [common.href],
    })
    const url = new URL(account)
    expect(url.origin).toBe('https://accounts.doubao.com')
    expect(url.searchParams.get('pc_version')).toBe('observed-pc_version')
    expect(url.searchParams.get('web_id')).toBeNull()
    expect(url.searchParams.get('sdk_version')).toBe('2.2.10')
    const captured = account + '&msToken=private&a_bogus=old'
    expect(run('doubaoBrowserAccountUrl(resources).href', { resources: [captured] })).toBe(account)
    expect(() => run('doubaoBrowserAccountUrl([])')).toThrow('尚未初始化')
  })

  it('豆包账号使用独立域名与账号参数，预检不泄漏 Cookie 且按实际 URL 缓存', () => {
    const requests = []
    const run = scripts(['_shared/api_session.js', 'doubao-api/scripts/auth.js'])
    const session = {
      cookies: [{ domain: '.doubao.com', name: 'session', value: 'private' }],
      auth: {
        fingerprint: {},
        account_query: { aid: '497858', account_sdk_source: 'web', sdk_version: '2.2.10' },
      },
    }
    run('host={call:callback}', {
      callback: (_action, request) => {
        requests.push(request)
        return {
          ok: true,
          status: 204,
          headers: {
            'access-control-allow-origin': 'https://www.doubao.com',
            'access-control-allow-credentials': 'true',
            'access-control-allow-methods': 'POST, GET',
            'access-control-allow-headers': 'content-type',
            'access-control-max-age': '600',
          },
        }
      },
    })
    run('session=input', { input: session })
    expect(run("doubaoRequestOrigin('/passport/account/info/v2/')")).toBe(
      'https://accounts.doubao.com'
    )
    expect(run("doubaoQuery(session,'/passport/account/info/v2/')")).toBe(
      'aid=497858&account_sdk_source=web&sdk_version=2.2.10'
    )
    run(
      "request={method:'GET',url:'https://accounts.doubao.com/passport/account/info/v2/?a_bogus=one'}; doubaoCorsPreflight(session,request,['content-type'],true); doubaoCorsPreflight(session,request,['content-type'],true)"
    )
    expect(requests).toHaveLength(1)
    expect(requests[0].headers.cookie).toBeUndefined()
    expect(requests[0].headers['content-type']).toBeUndefined()
    expect(requests[0].headers['access-control-request-headers']).toBe('content-type')
    run("doubaoCorsPreflight(session,{...request,url:request.url+'two'},['content-type'],true)")
    expect(requests).toHaveLength(2)
    expect(Object.keys(session)).not.toContain('corsPreflight')
    run('host.call=()=>({ok:true,status:204,headers:{}})')
    expect(() =>
      run("doubaoCorsPreflight(session,{...request,url:request.url+'three'},['content-type'],true)")
    ).toThrow('预检未允许')
  })

  it('保留账号语言优先级，并为 DeepSeek 会话使用对应的网页来源地址', () => {
    const run = scripts(['_shared/api_session.js', 'deepseek-api/scripts/auth.js'])
    const result = run(`deepseekHeaders({cookies:[],conversationId:'session-1',auth:{
      access_token:'test-token',fingerprint:{languages:['zh-CN','zh','en']}
    }},'/api/v0/chat/completion')`)
    expect(result['accept-language']).toBe('zh-CN,zh;q=0.9,en;q=0.8')
    expect(result.referer).toBe('https://chat.deepseek.com/a/chat/s/session-1')
    expect(result.accept).toBe('*/*')
  })

  it('保留多个 Set-Cookie，按路径发送并遵守删除与域限制', () => {
    const run = scripts(['_shared/api_session.js'])
    const session = { cookies: [{ name: 'old', value: 'stale', domain: '.doubao.com', path: '/' }] }
    const changed = run(
      `apiSessionUpdateCookies(session, headers, 'www.doubao.com', '/chat/completion', 'doubao.com')`,
      {
        session,
        headers: {
          'set-cookie': [
            'old=; Max-Age=0; Domain=.doubao.com; Path=/',
            'session=fresh; Domain=.doubao.com; Path=/; Expires=Wed, 21 Oct 2030 07:28:00 GMT; HttpOnly; Secure',
            'scope=chat; Path=/chat',
            'foreign=no; Domain=example.com; Path=/',
            'public=no; Domain=com; Path=/',
          ].join('\n'),
        },
      }
    )
    expect(changed).toBe(true)
    expect(run(`apiSessionCookies(session,'www.doubao.com','/chat/completion')`)).toBe(
      'session=fresh; scope=chat'
    )
    expect(run(`apiSessionCookies(session,'www.doubao.com','/chatother')`)).toBe('session=fresh')
    expect(run(`apiSessionCookies(session,'sub.doubao.com','/chat/completion')`)).toBe(
      'session=fresh'
    )
    expect(session.cookies).toHaveLength(2)
  })
})

describe('豆包正文与恢复协议', () => {
  it('思考摘要的子块和简写增量不混入回答，完成补丁不重复正文', () => {
    const output = []
    const run = scripts(
      [
        '_shared/api_session.js',
        'doubao-api/scripts/auth.js',
        'doubao-api/scripts/conversation.js',
      ],
      { checkpoint: () => ({}) }
    )
    run('state=doubaoStreamState(); onText=callback', { callback: (text) => output.push(text) })
    const block = (value) =>
      run(
        `doubaoConsume(state,{event:'STREAM_CHUNK',data:{patch_op:[{patch_object:1,patch_value:{content_block:[block]}}]}},onText)`,
        { block: value }
      )
    block({ block_id: 'think', block_type: 10040, content: { thinking_block: {} } })
    block({
      block_id: 'summary',
      parent_id: 'think',
      block_type: 10000,
      patch_type: 1,
      content: { text_block: { text: '计算' } },
    })
    run(`doubaoConsume(state,{event:'CHUNK_DELTA',data:{text:'过程'}},onText)`)
    block({
      block_id: 'answer',
      block_type: 10000,
      patch_type: 1,
      content: { text_block: { text: '4' } },
    })
    run(`doubaoConsume(state,{event:'CHUNK_DELTA',data:{text:'10'}},onText)`)
    block({
      block_id: 'answer',
      block_type: 10000,
      patch_type: 2,
      content: { text_block: { text: '410' } },
    })
    block({
      block_id: 'answer',
      block_type: 10000,
      patch_type: 1,
      is_finish: true,
      content: { text_block: {} },
    })
    expect(output.join('')).toBe('410')
    expect(run('state.text')).toBe('410')
    expect(run('state.blocks.summary.content.text_block.text')).toBe('计算过程')
  })

  it('创作目录按 next_cursor 查找旧视频，不遗漏后续页', () => {
    const requests = []
    const run = scripts(['doubao-api/scripts/history.js'])
    run('doubaoRequest=request', {
      request: (_session, path, body) => {
        requests.push({ path, body })
        if (path.endsWith('/homepage')) return { children: [{ name: '我的创作', id: 'folder' }] }
        if (path.endsWith('/get_download_info'))
          return { download_infos: [{ main_url: 'https://media.example/video.mp4' }] }
        return body.cursor
          ? { children: [{ key: 'target', id: 'video-node' }], has_more: false }
          : { children: [], has_more: true, next_cursor: 'next-page' }
      },
    })
    expect(run(`doubaoVideoDownload({},'target')`)).toBe('https://media.example/video.mp4')
    expect(requests[2].body.cursor).toBe('next-page')
    expect(requests[3].body).toEqual({ requests: [{ node_id: 'video-node' }] })
  })
})

describe('豆包人工验证失败路径', () => {
  it('STREAM_ERROR 验证挑战明确中止，不误报成功或重新生成', () => {
    const run = scripts(
      [
        '_shared/api_session.js',
        'doubao-api/scripts/auth.js',
        'doubao-api/scripts/conversation.js',
      ],
      { checkpoint: () => ({}) }
    )
    expect(() =>
      run(`doubaoConsume(doubaoStreamState(), {event:'STREAM_ERROR',data:{
      error_code:710022004,error_msg:'rate limited',extra:{decision:'private challenge'}
    }})`)
    ).toThrow('verification_required')
  })
})

describe('豆包多图的持久恢复', () => {
  it('第二张受理后中断，从原会话补齐并刷新第一张地址', () => {
    let checkpoint = {}
    let submitted = 0
    const host = {
      checkpoint: (value) => {
        if (value) checkpoint = structuredClone(value)
        return structuredClone(checkpoint)
      },
    }
    const run = scripts(
      ['doubao-api/scripts/auth.js', 'doubao-api/scripts/generate_image.js'],
      host
    )
    run(
      `apiSessionRead=()=>({auth:{}}); doubaoStartHeartbeats=()=>{}; doubaoRequest=()=>({}); doubaoUploadReferences=()=>[];
      doubaoMediaOptions=()=>({model:'Seedream 4.5',ratio:'auto'});
      doubaoReferenceBlocks=()=>[]; doubaoPayload=(prompt)=>({prompt});
      doubaoSubmit=submit; doubaoCreations=()=>[]; doubaoWaitForResult=history;
      doubaoMarkConversationRead=()=>{}`,
      {
        submit: () => {
          submitted++
          host.checkpoint({
            ...checkpoint,
            phase: 'accepted',
            conversation_id: 'conversation-' + submitted,
          })
          if (submitted === 2) throw Error('测试注入：已受理后中断')
          return { conversationId: 'conversation-' + submitted }
        },
        history: (_session, id) => [
          {
            image: {
              key: 'file-' + id,
              image_ori_raw: {
                url: 'https://media.example/refreshed-' + id,
                width: 1024,
                height: 1024,
              },
            },
          },
        ],
      }
    )
    expect(() => run(`main({prompt:'杯子',max_images:2})`)).toThrow('已受理后中断')
    expect(checkpoint.images).toHaveLength(1)
    const result = run(`main({prompt:'杯子',max_images:2})`)
    expect(submitted).toBe(2)
    expect(result.images.map((image) => image.conversation_id)).toEqual([
      'conversation-1',
      'conversation-2',
    ])
    expect(result.imageCount).toBe(2)
    expect(result.creative_prompt_receipt.prompts).toEqual(['杯子', '杯子'])
    expect(checkpoint.phase).toBe('completed')
  })
})

describe('豆包会话关联请求', () => {
  it('从平台能力列表取得图片和视频入口编号，不固定账号菜单编号', () => {
    const requests = []
    const run = scripts(['doubao-api/scripts/lifecycle.js'])
    run('doubaoRequest=send', {
      send: (_session, path, body) => {
        requests.push({ path, body })
        return path.endsWith('/brief_list')
          ? {
              entry_list: [
                { item_id: '1001', instruction_type: 4 },
                { item_id: '2002', instruction_type: 18 },
              ],
            }
          : {}
      },
    })
    for (const [ability, expected] of [
      [3, '1001'],
      [17, '2002'],
    ]) {
      run(
        `doubaoInitializeConversation({auth:{protocol_query:{language:'zh'}}}, {
        client_meta:{local_conversation_id:'local_test',bot_id:'test'},
        chat_ability:{ability_type:ability}
      })`,
        { ability }
      )
      expect(requests.at(-1)).toEqual({
        path: '/alice/slot/action_bar_v3/update_order',
        body: { bot_id: 'test', last_used_item_id: expected },
      })
      expect(requests.at(-2).body.item_ids).toEqual(['1001', '2002'])
    }
  })
})

describe('豆包媒体终态', () => {
  it('按网页会话回读协议读取最新消息，保留评估字段', () => {
    const run = scripts(['doubao-api/scripts/history.js'])
    let request
    run('apiTextUuid=()=>"request-id"; doubaoRequest=send', {
      send: (_session, path, body) => {
        request = { path, body }
        return {
          status_code: 0,
          downlink_body: { pull_singe_chain_downlink_body: { messages: [{ user_type: 2 }] } },
        }
      },
    })
    expect(run('doubaoHistory({},"123")')).toEqual([{ user_type: 2 }])
    expect(request.path).toBe('/im/chain/single')
    expect(request.body.uplink_body.pull_singe_chain_uplink_body).toEqual({
      conversation_id: '123',
      anchor_index: Number.MAX_SAFE_INTEGER,
      conversation_type: 3,
      direction: 1,
      limit: 20,
      ext: {},
      filter: { index_list: [] },
      evaluate_ab_params: '',
      evaluate_common_params: '',
      option: { lazy_load_strategy: 0 },
    })
  })

  it.each([
    ['image', { image: { status: 3 } }, 'content_policy_refusal'],
    ['image', { image: { status: 4 } }, 'platform_rejected'],
    ['video', { video: { status: 4 } }, 'platform_rejected'],
  ])('识别 %s 失败状态而不持续轮询', (kind, creation, error) => {
    const run = scripts(['doubao-api/scripts/history.js'], {
      sleep: () => {
        throw Error('不应等待')
      },
    })
    run('doubaoHistory=()=>messages', {
      messages: [
        {
          user_type: 2,
          content_block: [{ content: { creation_block: { creations: [creation] } } }],
        },
      ],
    })
    expect(() => run(`doubaoWaitForResult({},'123',kind)`, { kind })).toThrow(error)
  })
})

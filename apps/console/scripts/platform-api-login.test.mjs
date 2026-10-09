import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
const scriptDirectory = path.dirname(fileURLToPath(import.meta.url))
import { describe, expect, it } from 'vitest'
import { scripts } from './platform-api-test-helper.mjs'

describe('豆包人工验证状态', () => {
  it('保存服务端验证上下文，错误文本不携带验证凭据', () => {
    const writes = []
    const run = scripts(['doubao-api/scripts/auth.js'], {
      call: (action, value) => {
        writes.push({ action, ...value })
        return {}
      },
    })
    const session = { auth_path: '/account/auth.json', auth: {} }
    expect(() =>
      run(
        `doubaoCheckError({code:710022004,extra:{decision:JSON.stringify({
      type:'verify',subtype:'semantic_reasoning',detail:'private-challenge'
    })}},session)`,
        { session }
      )
    ).toThrow('verification_required')
    expect(writes[0].path).toBe('/account/verification.json')
    expect(writes[0].value.decision.detail).toBe('private-challenge')
    expect(writes[1].path).toBe('/account/auth.json')
    expect(session.auth.verification_required).toBe(true)
    expect(() => run('doubaoRequireVerified(session)')).toThrow('verification_required')
  })

  it('已有验证要求时，不上传附件也不提交新的生成任务', () => {
    let network = 0
    const run = scripts(['doubao-api/scripts/auth.js', 'doubao-api/scripts/generate_image.js'], {
      checkpoint: () => ({}),
      call: () => {
        network++
        throw Error('不应发送请求')
      },
    })
    run('apiSessionRead=()=>({auth:{verification_required:true}})')
    expect(() => run("main({prompt:'测试图片'})")).toThrow('verification_required')
    expect(network).toBe(0)
  })

  it('标准登录窗口只展示一次验证，服务端 SDK 确认成功后才检查登录', async () => {
    let rendered = 0
    let fetched = 0
    let options
    const run = scripts([])
    run(
      `window=globalThis; __pluginLoginContext={requires_manual_verification:true,decision:{type:'verify',code:'10000'}};
      verifySDK={autoRender:()=>{throw Error('图形验证应等待 renderCaptcha')},renderCaptcha:render}; performance={getEntriesByType:()=>[]};
      doubaoBrowserAccountUrl=()=>({href:'https://accounts.doubao.com/passport/account/info/v2/'});
      doubaoBrowserPassportCapture=()=>({});
      fetch=request`,
      {
        render: (value) => {
          rendered++
          options = value
        },
        request: async () => {
          fetched++
          return { ok: true, json: async () => ({ data: { user_id: 'test-user' } }) }
        },
      }
    )
    const source = readFileSync(
      path.resolve(scriptDirectory, '../../../plugins/doubao-api/scripts/check_login.js'),
      'utf8'
    )
    expect(JSON.parse(await run(source)).loggedIn).toBe(false)
    expect(JSON.parse(await run(source)).loggedIn).toBe(false)
    expect(rendered).toBe(1)
    expect(fetched).toBe(0)
    expect(options.commonOptions.aid).toBe(497858)
    expect(options.captchaOptions.showMode).toBe('mask')
    options.captchaOptions.successCb()
    expect(JSON.parse(await run(source)).loggedIn).toBe(true)
    expect(fetched).toBe(1)
  })

  it('验证码组件异步加载失败后允许下一次登录检测重试', async () => {
    const run = scripts([])
    let attempts = 0
    run(
      `window=globalThis; __pluginLoginContext={requires_manual_verification:true,decision:{code:10000}};
      verifySDK={autoRender:()=>{},renderCaptcha:render}`,
      {
        render: async () => {
          attempts++
          throw Error('组件加载失败')
        },
      }
    )
    const source = readFileSync(
      path.resolve(scriptDirectory, '../../../plugins/doubao-api/scripts/check_login.js'),
      'utf8'
    )
    expect(JSON.parse(await run(source)).loggedIn).toBe(false)
    expect(JSON.parse(await run(source)).loggedIn).toBe(false)
    expect(attempts).toBe(2)
  })
})

describe('平台登录结果遵守客户端标准契约', () => {
  it('身份有效但续期 SDK 未就绪时继续等待，不提前结束登录窗口', async () => {
    const run = scripts(['doubao-api/scripts/config.js'])
    run(
      `window=globalThis; URL=URLImpl; performance={getEntriesByType:()=>resources.map(name=>({name}))};
      resources=['https://accounts.doubao.com/passport/account/info/v2/?sdk_version=2.2.10'];
      fetch=async()=>({ok:true,json:async()=>({data:{user_id_str:'test-user'}})});`,
      { URLImpl: URL }
    )
    const source = readFileSync(
      path.resolve(scriptDirectory, '../../../plugins/doubao-api/scripts/check_login.js'),
      'utf8'
    )
    expect(JSON.parse(await run(source)).loggedIn).toBe(false)
    run(`__LOADABLE_LOADED_CHUNKS__=[[[],{config:function(){return {host:'https://accounts.doubao.com',isOversea:false,appKey:'2094abfcccefaeaffd24c622475fba10'}}}]];
      for(const path of ['/passport/token/beat/web/','/passport/doubao/security_token_beat/'])
        resources.push('https://accounts.doubao.com'+path+'?passport_jssdk_version=3.1.6&account_sdk_source_info=fp&sign=private');`)
    expect(JSON.parse(await run(source))).toMatchObject({ loggedIn: true, userId: 'test-user' })
    const capture = run('doubaoBrowserPassportCapture(resources)')
    expect(capture.passport_requests['/passport/token/beat/web/']).toEqual({
      passport_jssdk_version: '3.1.6',
      account_sdk_source_info: 'fp',
    })
    expect(capture.passport_app_key).toBe('2094abfcccefaeaffd24c622475fba10')
  })

  it('旧登录资料缺少续期协议属于资料更新，不是登录失效', () => {
    const run = scripts(['doubao-api/scripts/passport.js'])
    expect(() => run("doubaoPassportQuery({auth:{}},'/passport/token/beat/web/')")).toThrow(
      'AUTH_PROTOCOL_INCOMPLETE:'
    )
    expect(() => run("doubaoPassportQuery({auth:{}},'/passport/token/beat/web/')")).not.toThrow(
      'LOGIN_EXPIRED:'
    )
  })

  it.each(['doubao-api', 'deepseek-api'])(
    '%s 登录成功可被 Rust 检测并保留精确用户 ID',
    async (platform) => {
      const run = scripts([])
      run(
        `window=globalThis; performance={getEntriesByType:()=>[]};
      doubaoBrowserAccountUrl=()=>({href:'https://accounts.doubao.com/passport/account/info/v2/'});
      doubaoBrowserPassportCapture=()=>({});
      localStorage={getItem:()=>JSON.stringify({value:'test-token'})}; fetch=request`,
        {
          request: async () => ({
            ok: true,
            json: async () =>
              platform === 'doubao-api'
                ? { data: { user_id: 123, user_id_str: '90071992547409931', name: '测试' } }
                : { data: { biz_data: { id: '90071992547409931', name: '测试' } } },
          }),
        }
      )
      const raw = await run(
        readFileSync(
          path.resolve(scriptDirectory, `../../../plugins/${platform}/scripts/check_login.js`),
          'utf8'
        )
      )
      expect(typeof raw).toBe('string')
      const result = JSON.parse(raw)
      expect(result).toMatchObject({
        loggedIn: true,
        userId: '90071992547409931',
        username: '测试',
      })
      expect(result).not.toHaveProperty('logged_in')
    }
  )
})

describe('手机号和用户名映射', () => {
  const cases = [
    {
      platform: 'doubao-api',
      user: {
        user_id_str: 'db-user',
        name: '通行证名称',
        screen_name: '网页昵称',
        mobile: '199******31',
        email: '',
      },
      expected: { name: '网页昵称', contact: '199******31' },
    },
    {
      platform: 'deepseek-api',
      user: { id: 'ds-user', mobile_number: '199******31', email: '' },
      expected: { name: '199******31', contact: '199******31' },
    },
    {
      platform: 'deepseek-api',
      user: { id: 'ds-email', email: 'test@example.com' },
      expected: { name: 'test@example.com', contact: 'test@example.com' },
    },
    {
      platform: 'deepseek-api',
      user: { id: 'ds-name', name: '设置的昵称', mobile_number: '199******31' },
      expected: { name: '设置的昵称', contact: '199******31' },
    },
  ]
  it.each(cases)(
    '$platform 登录和状态刷新保留相同的账号资料',
    async ({ platform, user, expected }) => {
      const run = scripts([])
      run(
        `window=globalThis; performance={getEntriesByType:()=>[]};
      doubaoBrowserAccountUrl=()=>({href:'https://accounts.doubao.com/passport/account/info/v2/'});
      doubaoBrowserPassportCapture=()=>({});
      localStorage={getItem:()=>JSON.stringify({value:'test-token'})}; fetch=request`,
        {
          request: async () => ({
            ok: true,
            json: async () =>
              platform === 'doubao-api' ? { data: user } : { data: { biz_data: user } },
          }),
        }
      )
      const login = JSON.parse(
        await run(
          readFileSync(
            path.resolve(scriptDirectory, `../../../plugins/${platform}/scripts/check_login.js`),
            'utf8'
          )
        )
      )
      expect(login).toMatchObject({
        loggedIn: true,
        username: expected.name,
        email: expected.contact,
      })
      run(
        readFileSync(
          path.resolve(scriptDirectory, `../../../plugins/${platform}/scripts/check_account.js`),
          'utf8'
        )
      )
      run(`apiSessionRead=()=>({auth:{}});doubaoRequest=request;deepseekRequest=request`, {
        request: (_session, path) => (path.includes('overview') ? {} : user),
      })
      const health = run('main({})')
      expect(health).toMatchObject({
        logged_in: true,
        username: expected.name,
        name: expected.name,
        email: expected.contact,
      })
    }
  )
})

describe('豆包登录检测与凭据采集一致', () => {
  const resources = [
    'https://accounts.doubao.com/passport/account/info/v2/?aid=497858',
    'https://www.doubao.com/chat/?web_id=web&web_tab_id=tab',
    'https://www.doubao.com/bdms-sdk.test123.js',
    'https://accounts.doubao.com/passport/token/beat/web/?passport_jssdk_version=3.1.6&account_sdk_source_info=fp',
    'https://accounts.doubao.com/passport/doubao/security_token_beat/?passport_jssdk_version=4.4.6&account_sdk_source_info=fp',
  ]
  const capture = readFileSync(
    path.resolve(scriptDirectory, '../../../plugins/doubao-api/scripts/capture_auth.js'),
    'utf8'
  )
  const check = readFileSync(
    path.resolve(scriptDirectory, '../../../plugins/doubao-api/scripts/check_login.js'),
    'utf8'
  )
  function browser(user, ok = true, initialized = true) {
    const run = scripts(['doubao-api/scripts/config.js'])
    run(
      `window=globalThis;window.__LOADABLE_LOADED_CHUNKS__=[[0,{config:function(){return {getPassportSDKConfig:function(){},appKey:'2094abfcccefaeaffd24c622475fba10'}}}]];performance={getEntriesByType:()=>resources.map(name=>({name}))};
      document={cookie:'s_v_web_id=fingerprint'};
      localStorage={getItem:key=>key==='xmst' ? security : null};
      sessionStorage={getItem:()=>null}; apiLoginFingerprint=async()=>({});fetch=request`,
      {
        resources,
        URL,
        security: initialized ? 'private-security' : null,
        request: async () => ({ ok, json: async () => ({ data: user }) }),
      }
    )
    return run
  }
  it.each([
    { user_id_str: '90071992547409931', screen_name: '网页昵称', name: '账号名' },
    { user_id: 123, name: '旧版账号' },
    { user_id_str: '90071992547409931', user_id: 123, mobile: '199******31' },
  ])('接受字符串或旧版数字 ID，并优先保留精确字符串 ID', async (user) => {
    const run = browser(user)
    const login = JSON.parse(await run(check))
    const auth = JSON.parse(await run(capture))
    expect(login.loggedIn).toBe(true)
    expect(auth.user.user_id).toBe(login.userId)
    expect(auth.user.name).toBe(login.username)
    expect(auth.security.storage.xmst).toBe('private-security')
  })
  it('拒绝未登录、HTTP 错误及未初始化的安全会话', async () => {
    for (const [user, ok] of [
      [{}, true],
      [{ user_id_str: 'known' }, false],
    ]) {
      const run = browser(user, ok)
      expect(JSON.parse(await run(check)).loggedIn).toBe(false)
      await expect(run(capture)).rejects.toThrow('豆包登录状态尚未确认')
    }
    await expect(browser({ user_id_str: 'known' }, true, false)(capture)).rejects.toThrow(
      '豆包安全会话尚未初始化'
    )
  })
})

describe('豆包新版登录协议采集', () => {
  it('采集实际加载的新 SDK 和账号续期模板，移除旧签名并读取公开 SDK 配置', async () => {
    const run = scripts(['doubao-api/scripts/config.js'])
    const base = 'https://accounts.doubao.com'
    const resources = [
      base + '/passport/account/info/v2/?account_sdk_source=web&sdk_version=2.2.10',
      base +
        '/passport/token/beat/web/?passport_jssdk_version=3.1.6&account_sdk_source_info=fp&sign=old&qs=old&a_bogus=old&msToken=old&ts=old&scene=boot',
      base +
        '/passport/doubao/security_token_beat/?passport_jssdk_version=4.4.6&account_sdk_source_info=fp&sign=old',
      'https://lf-flow-web-cdn.doubao.com/obj/flow-doubao/rc-client-security/web/stable/1.0.1.20-alpha.08/bdms.js',
      'https://www.doubao.com/alice/test?web_id=web&web_tab_id=tab',
    ]
    run(
      `window=globalThis;window.__LOADABLE_LOADED_CHUNKS__=[[0,{config:function(){
      return {getPassportSDKConfig:function(){},appKey:'2094abfcccefaeaffd24c622475fba10'};
    }}]];performance={getEntriesByType:()=>resources.map(name=>({name}))};
    document={cookie:'s_v_web_id=profile'};
    localStorage={getItem:key=>key==='xmst'?'token':null};
    sessionStorage={getItem:()=>null};apiLoginFingerprint=async()=>({language:'zh-CN'});
    fetch=async()=>({ok:true,json:async()=>({data:{user_id:'test-user'}})})`,
      { resources, URL }
    )
    const source = readFileSync(
      path.resolve(scriptDirectory, '../../../plugins/doubao-api/scripts/capture_auth.js'),
      'utf8'
    )
    const auth = JSON.parse(await run(source))
    expect(auth.security.sdk_url).toBe(resources[3])
    expect(auth.passport_app_key).toBe('2094abfcccefaeaffd24c622475fba10')
    const template = auth.passport_requests['/passport/token/beat/web/']
    expect(template).toEqual({ passport_jssdk_version: '3.1.6', account_sdk_source_info: 'fp' })
    expect(auth.fingerprint.language).toBe('zh-CN')
    expect(auth.user.user_id).toBe('test-user')
  })
})

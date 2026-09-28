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
  it.each(['doubao-api', 'deepseek-api'])(
    '%s 登录成功可被 Rust 检测并保留精确用户 ID',
    async (platform) => {
      const run = scripts([])
      run(
        `window=globalThis; performance={getEntriesByType:()=>[]};
      doubaoBrowserAccountUrl=()=>({href:'https://accounts.doubao.com/passport/account/info/v2/'});
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

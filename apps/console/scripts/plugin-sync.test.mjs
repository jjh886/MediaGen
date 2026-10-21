import { describe, expect, it } from 'vitest'
import codex from '../../../plugins/codex/manifest.json'
import chatgpt from '../../../plugins/chatgpt/manifest.json'
import doubao from '../../../plugins/doubao-api/manifest.json'
import deepseek from '../../../plugins/deepseek-api/manifest.json'
import grok from '../../../plugins/grok/manifest.json'
import { scripts } from './platform-api-test-helper.mjs'

describe('新版插件装配与协议', () => {
  it('模型目录不再声明停用型号，保留无日期的图片型号和 Seedream 5', () => {
    const retired = [
      'gpt-image-1.5',
      'gpt-image-1-chatgpt',
      'gpt-image-2.5-sunburst-2026-09-08',
      'gpt-image-2.5-flare-2026-09-08',
      'doubao-seedream-4-5',
      'doubao-seedream-4-0',
      'grok-4.5',
      'grok-4.6',
      'grok-build-0.1',
      'grok-imagine-image',
    ]
    const ids = [chatgpt, codex, doubao, grok].flatMap((plugin) =>
      plugin.models.flatMap((model) => [model.model_id, ...(model.aliases || [])])
    )
    for (const id of retired) expect(ids).not.toContain(id)
    expect(ids).toEqual(
      expect.arrayContaining([
        'gpt-image-2.5-flare',
        'gpt-image-2.5-sunburst',
        'doubao-seedream-5-pro',
        'doubao-seedream-5-flash',
        'grok-4.7',
        'grok-imagine-image-quality',
        'grok-imagine-image-2.0',
      ])
    )
  })

  it('ChatGPT 刷新目录不会重新引入 5.6 之前的文本型号', () => {
    const retained = ['gpt-5-6', 'gpt-5-6-mini', 'gpt-5.6-pro', 'gpt-6-pro', 'gpt-6.1-sol']
    const retired = ['gpt-3.5', 'gpt-4', 'gpt-4o', 'gpt-5', 'gpt-5-5', 'gpt-5.5-pro', 'o3']
    const models = [...retained, ...retired, 'gpt-6-pro-wm', 'research'].map((slug) => ({ slug }))
    const run = scripts(['chatgpt/scripts/list_models.js'])
    run(
      'chatgptApiReadAuth=()=>({});chatgptApiRefreshSession=auth=>auth;chatgptApiRequireOk=value=>value;chatgptApiRequest=()=>({body:{models}});chatgptApiDiscoveredReasoning=()=>({})',
      { models: [...models, {}, null] }
    )
    expect(run('main({}).models').map((model) => model.model_id)).toEqual(retained)
  })

  it('生成流程包含续期、保活和 TTS 文本依赖，权限覆盖新增原子', () => {
    for (const name of ['generate_text', 'generate_image', 'generate_video'])
      expect(doubao.flows[name].steps[0].script_files).toContain('passport.js')
    expect(doubao.capabilities.actions).toEqual(
      expect.arrayContaining(['crypto.random_base64url', 'crypto.uuid_v1'])
    )
    expect(deepseek.flows.generate_text.steps[0].script_files).toContain('lifecycle.js')
    expect(chatgpt.flows.generate_audio.steps[0].script_files).toEqual(
      expect.arrayContaining(['network_profile.js', 'text_conversation.js'])
    )
    expect(codex.login_detection.script_files).toEqual(['auth.js', 'finish_oauth_login.js'])
  })

  it('Codex 登录按账号系统发送认证头，保存过期时间，失败不写入凭据', () => {
    const writes = []
    let accepted = true
    const run = scripts(['codex/scripts/auth.js', 'codex/scripts/finish_oauth_login.js'], {
      call: (action, params) => {
        if (action === 'http.request') {
          expect(params.headers.originator).toBe('codex-tui')
          expect(params.headers['user-agent']).toContain('Windows')
          expect(params.retry.max_attempts).toBe(1)
          return { ok: accepted, body: { access_token: 'test-token', expires_in: 3600 } }
        }
        if (action === 'file.write_json') return writes.push(params.value)
        throw new Error('未预期的原子调用：' + action)
      },
    })
    const ctx = { browser_os: 'windows', profile_dir: '/test', oauth: { code: 'test' } }
    expect(run('main(ctx)', { ctx }).account.api_token).toBe('test-token')
    expect(writes[0].expires_at).toBeGreaterThan(Date.now() / 1000 + 3590)
    accepted = false
    expect(() => run('main(ctx)')).toThrow('LOGIN_EXPIRED')
    expect(writes).toHaveLength(1)
    expect(run("codexAuthHeaders({account:{browser_os:'macos'}})['user-agent']")).toContain(
      'Mac OS'
    )
  })

  it('Codex 目录和请求保持一致，6.1 不接受 none，fast 使用 priority', () => {
    const models = ['gpt-6.1-sol', 'gpt-6-sol', 'gpt-5.5', 'gpt-5.4', 'gpt-4o', 'old-model'].map(
      (slug) => ({
        slug,
        supported_reasoning_levels: [{ effort: 'none' }, { effort: 'low' }, { effort: 'ultra' }],
        default_reasoning_level: 'low',
      })
    )
    const run = scripts(
      ['codex/scripts/auth.js', 'codex/scripts/text_responses.js', 'codex/scripts/list_models.js'],
      {
        call: () => ({ ok: true, body: { models } }),
        tryCall: () => ({ success: true, data: { ok: true, body: { models } } }),
      }
    )
    run('codexEnsureFreshAuth=()=>({tokens:{access_token:"test"}})')
    const discovered = run('main({}).models')
    expect(discovered.map((model) => model.model_id)).toEqual(['gpt-6.1-sol', 'gpt-6-sol'])
    expect(discovered[0].capabilities.reasoning.efforts).toEqual(['low'])
    expect(discovered[1].capabilities.reasoning.efforts).toEqual(['none', 'low'])
    expect(
      run(
        `codexNormalizeResponsesRequest({model:'gpt-6.1-sol',input:'你好',service_tier:'fast'},{}).payload.service_tier`
      )
    ).toBe('priority')
    expect(() =>
      run(`codexNormalizeResponsesRequest({model:'old-model',input:'你好'}, {})`)
    ).toThrow('unsupported_model')
    expect(() =>
      run(`codexApplyReasoning({}, {}, {model:'gpt-6.1-sol',reasoning:{effort:'none'}})`)
    ).toThrow('invalid_input')
    const image = scripts(['codex/scripts/direct_images.js'])
    expect(image('codexImageModel({model_id:"codex"})')).toBe('gpt-image-2.5-flare')
    expect(() => image('codexImageModel({model_id:"gpt-image-1.5"})')).toThrow('unsupported_model')
    for (const model of ['gpt-image-2.5-flare', 'gpt-image-2.5-sunburst']) {
      expect(image('codexImageModel({model_id:model})', { model })).toBe(model)
      expect(() => image('codexImageModel({model_id:model+"-2026-09-08"})', { model })).toThrow(
        'unsupported_model'
      )
    }
  })

  it('文本启动不访问图片启动接口，挑战页进入人工验证状态', () => {
    const paths = []
    const run = scripts(['chatgpt/scripts/network_profile.js', 'chatgpt/scripts/startup.js'])
    run(
      'chatgptApiFingerprint=()=>({});chatgptApiRequireOk=value=>value;chatgptApiRequest=request',
      {
        request: (_auth, _method, path) => {
          paths.push(path)
          return { ok: true, body: {} }
        },
      }
    )
    run('chatgptApiBootstrap({}, false)')
    expect(paths.some((path) => path.includes('/images/'))).toBe(false)
    expect(() =>
      run('chatgptApiRequireNoChallenge({headers:{"CF-Mitigated":"challenge"}}, "登录")')
    ).toThrow('verification_required')
    expect(() =>
      run('chatgptApiRequireNoChallenge({body_text:"window._cf_chl_opt={}"}, "登录")')
    ).toThrow('verification_required')
  })

  it('TTS 的 WAV/PCM/FLAC 经真实转码原子，转码失败不能返回成功', () => {
    const calls = []
    const run = scripts(['chatgpt/scripts/generate_audio.js'], {
      call: (action, params) => {
        calls.push({ action, ...params })
        return { base64: 'converted', mime_type: 'audio/' + params.format }
      },
    })
    run(
      'chatgptApiReadAuth=()=>({});chatgptApiRefreshSession=auth=>auth;chatgptApiGenerateSpeech=()=>({audio_base64:"source"})'
    )
    for (const format of ['wav', 'pcm', 'flac']) {
      const result = run('main({request:{input:"台词",response_format:format}})', { format })
      expect(result.audio_base64).toBe('converted')
      expect(result.format).toBe(format)
      expect(calls.at(-1)).toEqual({ action: 'audio.transcode', base64: 'source', format })
    }
    run('host.call=()=>{throw new Error("转码失败")}')
    expect(() => run('main({request:{input:"台词",response_format:"wav"}})')).toThrow('转码失败')
  })
})

describe('Codex 长上下文处理', () => {
  it('系统上下文只转换为 developer 一次，显式 instructions 保留', () => {
    const run = scripts(['codex/scripts/auth.js', 'codex/scripts/text_responses.js'])
    const content = '材料'.repeat(540000)
    const payload = run('codexNormalizeResponsesRequest(request, {}).payload', {
      request: {
        model: 'gpt-6-sol',
        instructions: '用户指定指令',
        input: [
          { role: 'system', content },
          { role: 'user', content: '继续' },
        ],
      },
    })
    expect(payload.instructions).toBe('用户指定指令')
    expect(payload.input[0]).toMatchObject({ role: 'developer', content })
  })
  it.each([200, 400])('上下文被明确拒绝时重置提交标记，HTTP %s 不影响分类', (status) => {
    const checkpoints = []
    const run = scripts(
      [
        'codex/scripts/auth.js',
        'codex/scripts/text_responses.js',
        'codex/scripts/generate_text.js',
      ],
      {
        checkpoint: (value) => {
          if (value) checkpoints.push(value)
          return {}
        },
        stream: (_action, _request, emit) => {
          emit({
            type: 'error',
            error: { code: 'context_length_exceeded', message: 'input exceeds the context window' },
          })
          return { ok: status === 200, status }
        },
      }
    )
    run('codexEnsureFreshAuth=()=>({});codexApplyReasoning=()=>{}')
    expect(() => run('main({request:{input:"继续"}})')).toThrow(
      'invalid_input: context_length_exceeded:'
    )
    expect(checkpoints.map((item) => item.phase)).toEqual(['submitting', 'ready'])
  })
})

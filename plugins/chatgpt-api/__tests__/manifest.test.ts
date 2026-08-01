import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const here = path.dirname(fileURLToPath(import.meta.url))
const apiManifest = JSON.parse(fs.readFileSync(path.resolve(here, '../manifest.json'), 'utf8'))
const browserManifest = JSON.parse(
  fs.readFileSync(path.resolve(here, '../../chatgpt/manifest.json'), 'utf8')
)

describe('chatgpt-api manifest', () => {
  it('作为独立 API 插件注册，不改变现有 ChatGPT 浏览器插件', () => {
    expect(apiManifest.platform_id).toBe('chatgpt-api')
    expect(apiManifest.adapter_mode).toBe('api')
    expect(browserManifest.platform_id).toBe('chatgpt')
    expect(browserManifest.adapter_mode).toBe('browser')
  })

  it('只在登录完成时采集凭据，生成流程不包含浏览器动作', () => {
    expect(apiManifest.login_credential_capture).toMatchObject({
      script_file: 'capture_auth.js',
      file_name: 'auth.json',
    })
    const actions = apiManifest.flows.generate_image.steps.map(
      (step: { action: string }) => step.action
    )
    expect(actions).toEqual(['plugin.run_js'])
  })

  it('为十分钟生成期限保留一分钟错误回传时间', () => {
    const step = apiManifest.flows.generate_image.steps[0]

    expect(step.timeout_ms).toBe(11 * 60 * 1000)
  })

  it('在插件内声明生成配置，通用服务和前端不写死平台字段', () => {
    expect(apiManifest.generation_settings.title).toBe('ChatGPT API 生成配置')
    expect(apiManifest.generation_settings.fields).toMatchObject([
      { key: 'model', type: 'text', default: '' },
      {
        key: 'thinking_effort',
        type: 'select',
        default: 'extended',
        options: [{ value: '' }, { value: 'standard' }, { value: 'extended' }],
      },
    ])
  })
})

import fs from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const here = path.dirname(fileURLToPath(import.meta.url))
const scriptDir = path.resolve(here, '../scripts')
const runtimeScript = ['auth.js', 'conversation.js', 'generate_image.js']
  .map((name) => fs.readFileSync(path.join(scriptDir, name), 'utf8'))
  .join('\n')

type JsonValue = Record<string, unknown> | unknown[] | string | number | boolean | null

function evaluate(expression: string): JsonValue {
  // 通过 JSON 往返消除 VM realm 差异，让断言只关注插件对外产生的数据。
  const serialized = vm.runInNewContext(
    `${runtimeScript}\nJSON.stringify(${expression})`,
    createSandbox()
  )
  return JSON.parse(String(serialized)) as JsonValue
}

function evaluateError(expression: string): string {
  return String(
    vm.runInNewContext(
      `${runtimeScript}\ntry { ${expression} } catch (error) { error.message }`,
      createSandbox()
    )
  )
}

function createSandbox() {
  return {
    host: {
      call: () => {
        throw new Error('这个单元测试不应发起真实主机请求')
      },
    },
  }
}

const auth = `{
  runtime_started_at: Date.now(),
  cookies: [{ name: 'session', value: 'cookie' }, { name: 'oai-did', value: 'device' }],
  auth: {
    access_token: 'secret',
    fingerprint: {
      user_agent: 'ChromeUA',
      language: 'en-US',
      languages: ['en-US', 'en'],
      timezone: 'America/Los_Angeles',
      timezone_offset_min: 420,
      page_build: 'prod-build',
      page_sequence: '123',
      ua_brands: [{ brand: 'Google Chrome', version: '145' }],
      ua_full_version: '145.0.1.2',
      ua_architecture: 'arm',
      ua_bitness: '64',
      ua_mobile: false,
      ua_platform: 'macOS'
    }
  }
}`

describe('chatgpt-api conversation contract', () => {
  it('复用登录浏览器的 UA 和客户端指纹请求头', () => {
    const headers = evaluate(
      `chatgptApiBaseHeaders(${auth}, '/backend-api/models', '/backend-api/models')`
    ) as Record<string, unknown>

    expect(headers['user-agent']).toBe('ChromeUA')
    expect(headers['sec-ch-ua-arch']).toBe('"arm"')
    expect(headers['sec-ch-ua-full-version']).toBe('"145.0.1.2"')
    expect(headers['oai-client-version']).toBe('prod-build')
    expect(headers['oai-client-build-number']).toBe('123')
    expect(headers['chatgpt-account-id']).toBeUndefined()
  })

  it('参考图使用当前多模态消息结构，并把思考深度传给两次请求', () => {
    const result = evaluate(`(() => {
      const references = [{
        file_id: 'file_123',
        library_file_id: 'libfile_123',
        name: 'reference.png',
        mime_type: 'image/png',
        size: 318862,
        width: 1024,
        height: 1024
      }]
      return {
        prepare: chatgptApiConversationPreparePayload(
          ${auth}, 'gpt-5-6-thinking', references, 'file_picker', 'extended'
        ),
        message: chatgptApiConversationPayload(
          ${auth}, 'gpt-5-6-thinking', 'keep the subject', '1:1', references, 'extended'
        )
      }
    })()`) as { prepare: Record<string, unknown>; message: Record<string, unknown> }

    expect(result.prepare.attachment_mime_types).toEqual(['image/png'])
    expect(result.prepare.client_prepare_source).toBe('file_picker')
    expect(result.prepare.thinking_effort).toBe('extended')
    expect(result.message).toMatchObject({
      model: 'gpt-5-6-thinking',
      thinking_effort: 'extended',
      messages: [
        {
          content: {
            content_type: 'multimodal_text',
            parts: [{ asset_pointer: 'sediment://file_123' }, expect.any(String)],
          },
          metadata: {
            attachments: [{ library_file_id: 'libfile_123' }],
          },
        },
      ],
    })
  })

  it('模型和思考深度只接受当前账号能力内的配置', () => {
    const bootstrap = `{
      '/backend-api/models': { models: [{ slug: 'auto' }, { slug: 'gpt-5-5' }] }
    }`
    const configured = evaluate(
      `chatgptApiGenerationConfig(
        { platform_config: { model: 'gpt-5-5', thinking_effort: 'standard' } },
        ${bootstrap}
      )`
    )
    const unavailable = evaluateError(
      `chatgptApiGenerationConfig(
        { platform_config: { model: 'gpt-not-available', thinking_effort: 'standard' } },
        ${bootstrap}
      )`
    )

    expect(configured).toEqual({ model: 'gpt-5-5', thinking_effort: 'standard' })
    expect(unavailable).toContain('当前 ChatGPT 账号不可用模型')
  })

  it('空思考深度不写入请求，让平台使用默认值', () => {
    const payload = evaluate(
      `chatgptApiConversationPreparePayload(${auth}, 'auto', [], 'composer_editor_state', '')`
    ) as Record<string, unknown>

    expect(payload.thinking_effort).toBeUndefined()
  })

  it('只从平台回复中识别生成文件，不会把用户参考图当成结果', () => {
    const fileId = evaluate(`chatgptApiGeneratedFileId({ mapping: {
      input: { message: {
        id: 'input', author: { role: 'user' },
        content: { parts: [{ asset_pointer: 'sediment://file_reference' }] }
      } },
      output: { message: {
        id: 'output', author: { role: 'tool' },
        content: { parts: [{ asset_pointer: 'sediment://file_generated' }] }
      } }
    } })`)

    expect(fileId).toBe('file_generated')
  })

  it('账号凭据只发送给 ChatGPT 同源下载地址', () => {
    const headers = evaluate(`({
      same: chatgptApiDownloadImageHeaders(
        ${auth}, 'https://chatgpt.com/backend-api/estuary/content?id=file'
      ),
      external: chatgptApiDownloadImageHeaders(
        ${auth}, 'https://files.oaiusercontent.com/file.png'
      )
    })`) as {
      same: Record<string, unknown>
      external: Record<string, unknown>
    }

    expect(headers.same.authorization).toBe('Bearer secret')
    expect(headers.same.cookie).toContain('session=cookie')
    expect(headers.external.authorization).toBeUndefined()
    expect(headers.external.cookie).toBeUndefined()
  })

  it('下载地址尚未就绪时继续轮询，其他错误立即暴露', () => {
    expect(
      evaluate(`[404, 409, 425, 403].map((status) =>
        chatgptApiImageDownloadPending({ status })
      )`)
    ).toEqual([true, true, true, false])
  })
})

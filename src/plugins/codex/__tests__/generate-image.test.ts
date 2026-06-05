import fs from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const here = path.dirname(fileURLToPath(import.meta.url))
const scriptPath = path.resolve(here, '../../../../plugins/codex/scripts/generate_image.js')
const authScriptPath = path.resolve(here, '../../../../plugins/codex/scripts/auth.js')
const retryScriptPath = path.resolve(here, '../../../../plugins/codex/scripts/retry.js')
const checkAccountScriptPath = path.resolve(
  here,
  '../../../../plugins/codex/scripts/check_account.js'
)
const script = fs.readFileSync(scriptPath, 'utf8')
const authScript = fs.readFileSync(authScriptPath, 'utf8')
const retryScript = fs.readFileSync(retryScriptPath, 'utf8')
const checkAccountScript = fs.readFileSync(checkAccountScriptPath, 'utf8')
const imageRuntimeScript = retryScript + '\n' + script

type ImageTool = Record<string, unknown>

function buildPayloadFromCtx(ctx: Record<string, unknown>) {
  const sandbox = {
    ctx,
    host: {
      call: () => {
        throw new Error('测试里没有传本地参考图，不应该读取文件。')
      },
    },
  }
  const raw = vm.runInNewContext(
    imageRuntimeScript + '\nJSON.stringify(buildPayload(ctx, ctx.prompt, buildRequest(ctx)))',
    sandbox
  )
  return JSON.parse(String(raw)) as { tools: ImageTool[] }
}

function extractImageFromEvents(events: unknown[]) {
  const sandbox = { events }
  const raw = vm.runInNewContext(
    imageRuntimeScript + '\nJSON.stringify(extractImage(events))',
    sandbox
  )
  return JSON.parse(String(raw)) as {
    partial_images: Array<Record<string, unknown>>
  }
}

function jwt(payload: Record<string, unknown>) {
  const body = Buffer.from(JSON.stringify(payload), 'utf8').toString('base64url')
  return `header.${body}.signature`
}

describe('codex image bridge script', () => {
  it('forwards OpenAI image API params into the Codex image tool', () => {
    const payload = buildPayloadFromCtx({
      prompt: '保持原商品，换成厨房台面背景',
      max_images: 2,
      size: '1024x1536',
      quality: 'medium',
      background: 'transparent',
      output_format: 'webp',
      output_compression: 72,
      partial_images: 3,
      moderation: 'low',
      input_fidelity: 'high',
      image_action: 'edit',
    })

    expect(payload.tools[0]).toEqual({
      type: 'image_generation',
      size: '1024x1536',
      quality: 'medium',
      background: 'transparent',
      output_format: 'webp',
      action: 'edit',
      output_compression: 72,
      partial_images: 3,
      moderation: 'low',
      input_fidelity: 'high',
    })
  })

  it('keeps API size auto instead of guessing a fixed platform size', () => {
    const payload = buildPayloadFromCtx({
      prompt: 'A simple product photo',
      size: 'auto',
    })

    expect(payload.tools[0].size).toBe('auto')
  })

  it('adds edit mask when API passes a mask image data URL', () => {
    const payload = buildPayloadFromCtx({
      prompt: '只替换蒙版区域',
      image_action: 'edit',
      mask_image_dataurl: 'data:image/png;base64,bWFzaw==',
    })

    expect(payload.tools[0].input_image_mask).toEqual({
      image_url: 'data:image/png;base64,bWFzaw==',
    })
  })

  it('collects upstream partial image events for local SSE output', () => {
    const result = extractImageFromEvents([
      {
        type: 'response.image_generation_call.partial_image',
        partial_image_index: 2,
        partial_image_b64: 'cGFydGlhbA==',
        output_format: 'png',
      },
    ])

    expect(result.partial_images).toEqual([
      {
        partial_image_index: 2,
        b64_json: 'cGFydGlhbA==',
        output_format: 'png',
      },
    ])
  })

  it('retries transient Codex network errors before giving up', () => {
    const sleeps: number[] = []
    let calls = 0
    const sandbox = {
      host: {
        call: () => {
          calls += 1
          if (calls === 1) throw new Error('error decoding response body: unexpected EOF')
          return {
            events: [
              {
                type: 'response.output_item.done',
                item: { type: 'image_generation_call', result: 'aW1hZ2U=' },
              },
            ],
          }
        },
        sleep: (ms: number) => sleeps.push(ms),
      },
    }

    const raw = vm.runInNewContext(
      imageRuntimeScript + "\nJSON.stringify(requestImageWithRetry('access', 'acct', {}))",
      sandbox
    )
    const result = JSON.parse(String(raw)) as { image: string }

    expect(result.image).toBe('aW1hZ2U=')
    expect(calls).toBe(2)
    expect(sleeps).toEqual([5000])
  })

  it('refreshes Codex auth before using an almost expired access token', () => {
    const auth = {
      tokens: {
        access_token: jwt({ exp: Math.floor(Date.now() / 1000) + 60 }),
        refresh_token: 'refresh-old',
      },
    }
    let written: unknown
    const sandbox = {
      ctx: { profile_dir: '/profile', account: {} },
      host: {
        call: (name: string, args: Record<string, unknown>) => {
          if (name === 'file.read_json') return auth
          if (name === 'file.write_json') {
            written = args.value
            return null
          }
          if (name === 'crypto.base64url_decode_utf8') {
            return { value: Buffer.from(String(args.text), 'base64url').toString('utf8') }
          }
          throw new Error(`unexpected host.call ${name}`)
        },
        tryCall: () => ({
          success: true,
          data: {
            status: 200,
            body: {
              access_token: jwt({ exp: Math.floor(Date.now() / 1000) + 3600 }),
              refresh_token: 'refresh-new',
            },
          },
        }),
      },
    }

    const raw = vm.runInNewContext(
      authScript + '\nJSON.stringify(codexEnsureFreshAuth(ctx))',
      sandbox
    )
    const refreshed = JSON.parse(String(raw)) as { tokens: Record<string, string> }

    expect(refreshed.tokens.refresh_token).toBe('refresh-new')
    expect(written).toEqual(refreshed)
  })

  it('returns available health from the Codex account check script', () => {
    const accessToken = jwt({ exp: Math.floor(Date.now() / 1000) + 3600, email: 'id@example.com' })
    const sandbox = {
      ctx: { profile_dir: '/profile', account: {} },
      host: {
        call: (name: string, args: Record<string, unknown>) => {
          if (name === 'file.read_json') {
            return { tokens: { access_token: accessToken, refresh_token: 'refresh' } }
          }
          if (name === 'crypto.base64url_decode_utf8') {
            return { value: Buffer.from(String(args.text), 'base64url').toString('utf8') }
          }
          throw new Error(`unexpected host.call ${name}`)
        },
        tryCall: () => ({
          success: true,
          data: {
            status: 200,
            body: { account_email: 'profile@example.com', user_id: 'acct_123' },
          },
        }),
      },
    }

    const raw = vm.runInNewContext(
      authScript + '\n' + checkAccountScript + '\nJSON.stringify(main(ctx))',
      sandbox
    )
    const health = JSON.parse(String(raw)) as {
      status: string
      account: Record<string, string>
    }

    expect(health.status).toBe('available')
    expect(health.account.email).toBe('profile@example.com')
    expect(health.account.user_id).toBe('acct_123')
  })
})

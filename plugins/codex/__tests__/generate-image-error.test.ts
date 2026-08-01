import fs from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const here = path.dirname(fileURLToPath(import.meta.url))
const scriptDir = path.resolve(here, '../scripts')
const runtimeScript =
  fs.readFileSync(path.join(scriptDir, 'retry.js'), 'utf8') +
  '\n' +
  fs.readFileSync(path.join(scriptDir, 'image_response.js'), 'utf8') +
  '\n' +
  fs.readFileSync(path.join(scriptDir, 'generate_image.js'), 'utf8')

function extractImage(events: unknown[]) {
  const raw = vm.runInNewContext(runtimeScript + '\nJSON.stringify(extractImage(events))', {
    events,
  })
  return JSON.parse(String(raw)) as { error: string; output_text: string }
}

function requestError(response: unknown) {
  const sandbox = {
    host: { call: () => response, sleep: () => undefined },
  }
  return String(
    vm.runInNewContext(
      runtimeScript +
        "\ntry { requestImageWithRetry('access', 'account', {}) } catch (error) { error.message }",
      sandbox
    )
  )
}

describe('codex image upstream error extraction', () => {
  it('returns the concrete output text when the API completes without an image', () => {
    const result = extractImage([
      {
        type: 'response.output_item.done',
        item: { type: 'image_generation_call', status: 'completed' },
      },
      { type: 'response.output_text.delta', delta: 'Image generation ' },
      { type: 'response.output_text.delta', delta: 'is unavailable for this request.' },
      {
        type: 'response.output_text.done',
        text: 'Image generation is unavailable for this request.',
      },
      {
        type: 'response.completed',
        response: {
          output: [
            {
              type: 'message',
              content: [
                { type: 'output_text', text: 'Image generation is unavailable for this request.' },
              ],
            },
          ],
        },
      },
    ])

    expect(result.error).toBe(
      'Codex API 返回文本：Image generation is unavailable for this request.'
    )
  })

  it('keeps the API error code and message from a failed response', () => {
    const result = extractImage([
      {
        type: 'response.failed',
        response: {
          error: {
            code: 'safety_violations',
            message: 'The request was rejected by the safety system.',
          },
        },
      },
    ])

    expect(result.error).toBe('safety_violations: The request was rejected by the safety system.')
  })

  it('compresses repeated event names when the API provides no detail', () => {
    const result = extractImage(Array.from({ length: 80 }, () => ({ type: 'keepalive' })))

    expect(result.error).toBe('No image_generation_call.result found. events: keepalive×80')
    expect(result.error.length).toBeLessThan(200)
  })

  it('returns the HTTP error body when the API does not send SSE events', () => {
    const message = requestError({
      ok: false,
      status: 400,
      events: [],
      body: { error: { code: 'invalid_request', message: 'The prompt is invalid.' } },
    })

    expect(message).toBe('HTTP 400 invalid_request: The prompt is invalid.')
  })
})

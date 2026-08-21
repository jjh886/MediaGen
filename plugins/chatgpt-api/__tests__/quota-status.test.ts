import fs from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const here = path.dirname(fileURLToPath(import.meta.url))
const script = fs.readFileSync(path.resolve(here, '../scripts/quota_status.js'), 'utf8')

function runExpression<T>(expression: string): T {
  return vm.runInNewContext(`${script}\n${expression}`) as T
}

describe('chatgpt api quota status parsing', () => {
  it('extracts the real Pro image reset message and duration', () => {
    const message = runExpression<string>(
      `chatgptApiQuotaMessageFromText(${JSON.stringify(
        "You've hit the Pro plan limit for image generations requests. " +
          'You can create more images when the limit resets in 4 hours and 35 minutes.'
      )})`
    )

    expect(message).toContain('limit resets in 4 hours and 35 minutes')
    expect(runExpression<number>(`chatgptApiDurationSeconds(${JSON.stringify(message)})`)).toBe(
      16_500
    )
  })

  it('supports minutes, days, and Chinese units without platform-specific Rust rules', () => {
    expect(runExpression<number>("chatgptApiDurationSeconds('try again in 12 minutes')")).toBe(720)
    expect(runExpression<number>("chatgptApiDurationSeconds('limit resets in 1 day')")).toBe(86_400)
    expect(runExpression<number>("chatgptApiDurationSeconds('请在 2小时30分钟 后重试')")).toBe(
      9_000
    )
  })

  it('does not hide an explicit login expiry from the account status refresh', () => {
    expect(() =>
      runExpression(`(() => {
        chatgptApiBootstrap = () => { throw new Error('LOGIN_EXPIRED: /backend-api/me HTTP 401') }
        return chatgptApiQuotaMessageFromBootstrap({})
      })()`)
    ).toThrow('LOGIN_EXPIRED')
  })

  it('still treats temporary quota lookup errors as an unavailable quota detail', () => {
    expect(
      runExpression(`(() => {
        chatgptApiBootstrap = () => { throw new Error('AUTH_INCONCLUSIVE: request timeout') }
        return chatgptApiQuotaMessageFromBootstrap({})
      })()`)
    ).toBe('')
  })
})

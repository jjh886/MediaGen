import { describe, expect, it } from 'vitest'
import { accountResetText } from './account-status'

describe('accountResetText', () => {
  it('shows reset time only for temporary unavailable account states', () => {
    const cooldownUntil = '2026-08-14T20:35:00.000Z'

    expect(
      accountResetText({ login_state: 'daily_quota_exhausted', cooldown_until: cooldownUntil })
    ).toContain('额度恢复时间')
    expect(accountResetText({ login_state: 'cooldown', cooldown_until: cooldownUntil })).toContain(
      '额度恢复时间'
    )
    expect(accountResetText({ login_state: 'available', cooldown_until: cooldownUntil })).toBe('')
  })
})

import { describe, expect, it } from 'vitest'
import { formatDateTime } from './date-time'

describe('固定 UTC+8 时间显示', () => {
  it.each([
    '2026-12-31T17:30:00Z',
    '2026-12-31T17:30:00+00:00',
    '2027-01-01T01:30:00+08:00',
    '2026-12-31T13:30:00-04:00',
  ])('正确转换时区与跨年日期：%s', (value) => {
    expect(formatDateTime(value)).toBe('2027/01/01 01:30:00')
    expect(formatDateTime(value, true)).toBe('01/01 01:30')
  })

  it('午夜使用 00 点，历史时间也固定 +8，不套用夏令时', () => {
    expect(formatDateTime('2024-02-28T16:00:00Z')).toBe('2024/02/29 00:00:00')
    expect(formatDateTime('1990-07-01T00:00:00Z')).toBe('1990/07/01 08:00:00')
  })

  it.each([undefined, null, '', 'invalid-date'])('缺失或无效时间安全显示占位符', (value) => {
    expect(formatDateTime(value)).toBe('-')
    expect(formatDateTime(value, true)).toBe('-')
  })
})

import { describe, expect, it } from 'vitest'
import { safeRemoteCursor } from '../remote-browser-cursor'

describe('safeRemoteCursor', () => {
  it('keeps browser text and interaction cursors', () => {
    expect(safeRemoteCursor('text')).toBe('text')
    expect(safeRemoteCursor('pointer')).toBe('pointer')
    expect(safeRemoteCursor('grabbing')).toBe('grabbing')
  })

  it('falls back for unsafe custom cursor values', () => {
    expect(safeRemoteCursor('url(https://example.com/cursor.png)')).toBe('default')
    expect(safeRemoteCursor('')).toBe('default')
  })
})

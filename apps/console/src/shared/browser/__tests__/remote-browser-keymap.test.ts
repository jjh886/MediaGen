import type { KeyboardEvent } from 'react'
import { describe, expect, it } from 'vitest'
import { editingCommands, remoteShortcut } from '../remote-browser-keymap'

function keyEvent(overrides: Partial<KeyboardEvent<HTMLElement>>): KeyboardEvent<HTMLElement> {
  return {
    key: '',
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    ...overrides,
  } as KeyboardEvent<HTMLElement>
}

describe('remote browser keymap', () => {
  it('maps Windows and macOS clipboard shortcuts', () => {
    expect(remoteShortcut(keyEvent({ ctrlKey: true, key: 'c' }))).toBe('copy')
    expect(remoteShortcut(keyEvent({ metaKey: true, key: 'v' }))).toBe('paste')
    expect(remoteShortcut(keyEvent({ ctrlKey: true, key: 'x' }))).toBe('cut')
    expect(remoteShortcut(keyEvent({ metaKey: true, key: 'a' }))).toBe('select_all')
  })

  it('maps browser navigation and editing commands', () => {
    expect(remoteShortcut(keyEvent({ altKey: true, key: 'ArrowLeft' }))).toBe('back')
    expect(remoteShortcut(keyEvent({ ctrlKey: true, key: 'r' }))).toBe('reload')
    expect(editingCommands(keyEvent({ metaKey: true, shiftKey: true, key: 'z' }))).toEqual(['redo'])
    expect(editingCommands(keyEvent({ ctrlKey: true, key: 'z' }))).toEqual(['undo'])
  })
})

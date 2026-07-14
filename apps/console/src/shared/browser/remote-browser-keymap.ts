import type { KeyboardEvent } from 'react'

export type RemoteShortcut =
  | 'back'
  | 'copy'
  | 'cut'
  | 'forward'
  | 'paste'
  | 'reload'
  | 'select_all'
  | null

const COMMAND_SHORTCUTS: Record<string, Exclude<RemoteShortcut, null>> = {
  a: 'select_all',
  c: 'copy',
  r: 'reload',
  v: 'paste',
  x: 'cut',
}

const ALT_SHORTCUTS: Record<string, Exclude<RemoteShortcut, null>> = {
  arrowleft: 'back',
  arrowright: 'forward',
}

export function remoteShortcut(event: KeyboardEvent<HTMLElement>): RemoteShortcut {
  const key = event.key.toLowerCase()
  if (event.metaKey || event.ctrlKey) return COMMAND_SHORTCUTS[key] ?? null
  if (event.altKey) return ALT_SHORTCUTS[key] ?? null
  if (key === 'f5') return 'reload'
  return null
}

export function editingCommands(event: KeyboardEvent<HTMLElement>): string[] {
  const command = event.metaKey || event.ctrlKey
  const key = event.key.toLowerCase()
  if (!command) return []
  if (key === 'z' && event.shiftKey) return ['redo']
  if (key === 'z') return ['undo']
  if (key === 'y') return ['redo']
  return []
}

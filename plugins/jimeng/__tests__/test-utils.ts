// Shared helpers for browser-evaluated plugin scripts. They rely on the Vitest
// jsdom + Node test runtime and are not production frontend code.
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { vi } from 'vitest'

const here = path.dirname(fileURLToPath(import.meta.url))
const scriptsDir = path.resolve(here, '../scripts')

export function readScript(name: string) {
  return fs.readFileSync(path.join(scriptsDir, name), 'utf8')
}

export async function runScriptJson(name: string) {
  const raw = window.eval(readScript(name))
  const value = raw instanceof Promise ? await raw : raw
  return JSON.parse(String(value))
}

export function setJimengState(value: unknown) {
  const holder = window as unknown as Record<symbol, unknown>
  holder[Symbol.for('_jimeng')] = value
}

export function getJimengState() {
  const holder = window as unknown as Record<symbol, Record<string, unknown>>
  return holder[Symbol.for('_jimeng')]
}

export function markVisible(el: Element | null) {
  if (!el) return
  Object.defineProperty(el, 'getBoundingClientRect', {
    configurable: true,
    value: () => ({
      width: 120,
      height: 32,
      x: 0,
      y: 0,
      top: 0,
      left: 0,
      right: 120,
      bottom: 32,
      toJSON: () => ({}),
    }),
  })
}

export function resetJimengDom() {
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  vi.useRealTimers()
}

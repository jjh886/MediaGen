import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const scriptsDir = path.resolve(here, '../../plugins/chatgpt/scripts')

function readScript(name: string) {
  return fs.readFileSync(path.join(scriptsDir, name), 'utf8')
}

export function setCgptState(value: unknown) {
  const holder = window as unknown as { [key: symbol]: unknown }
  holder[Symbol.for('_cgpt')] = value
}

export function getCgptState<T = Record<string, unknown>>() {
  const holder = window as unknown as Record<symbol, T>
  return holder[Symbol.for('_cgpt')]
}

export function markVisible(el: Element | null, rect = { width: 120, height: 32, x: 0, y: 0 }) {
  if (!el) return
  Object.defineProperty(el, 'getBoundingClientRect', {
    configurable: true,
    value: () => ({
      width: rect.width,
      height: rect.height,
      x: rect.x,
      y: rect.y,
      top: rect.y,
      left: rect.x,
      right: rect.x + rect.width,
      bottom: rect.y + rect.height,
      toJSON: () => ({}),
    }),
  })
}

export async function runScriptJson(name: string) {
  // 这些脚本依赖共享 DOM 工具，测试时需要按真实 manifest 顺序先注入。
  if (
    name === 'probe_image_mode.js' ||
    name === 'select_ratio.js' ||
    name === 'wait_ratio_menu_option.js'
  ) {
    window.eval(readScript('dom_helpers.js'))
  }
  const raw = window.eval(readScript(name))
  const value = raw instanceof Promise ? await raw : raw
  return JSON.parse(String(value))
}

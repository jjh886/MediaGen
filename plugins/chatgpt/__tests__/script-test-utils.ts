import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const scriptsDir = path.resolve(here, '../scripts')

function readScript(name: string) {
  return fs.readFileSync(path.join(scriptsDir, name), 'utf8')
}

export function setCgptState(value: unknown) {
  const holder = window as unknown as { [key: symbol]: unknown }
  holder[Symbol.for('_cgpt')] = value
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
  // 使用和生产环境相同的脚本内容执行，避免测试复制一份已经过时的选择器逻辑。
  const raw = window.eval(readScript(name))
  const value = raw instanceof Promise ? await raw : raw
  return JSON.parse(String(value))
}

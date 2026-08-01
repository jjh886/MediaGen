#!/usr/bin/env node

/**
 * Rust 平台边界检查。
 *
 * 全部 Rust 代码（包括测试）只能实现通用原子能力、服务编排和公共协议适配，不允许把
 * 具体插件名、站点域名或供应商上传细节写进通用模块。平台差异必须放到插件中。
 */

import { promises as fs } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const CURRENT_FILE = fileURLToPath(import.meta.url)
const REPO_ROOT = path.resolve(path.dirname(CURRENT_FILE), '..')
const RUST_ROOTS = [
  path.join(REPO_ROOT, 'apps', 'server', 'src'),
  path.join(REPO_ROOT, 'apps', 'local-launcher', 'src'),
  path.join(REPO_ROOT, 'crates'),
]

const BANNED_PATTERNS = [
  { label: 'chatgpt platform id', pattern: /\bchatgpt\b/i },
  { label: 'codex platform id', pattern: /\bcodex\b/i },
  { label: 'doubao platform id', pattern: /\bdoubao\b/i },
  { label: 'jimeng platform id', pattern: /\bjimeng\b/i },
  { label: 'chinese platform name', pattern: /豆包|即梦|剪映/ },
  { label: 'platform domain', pattern: /\b(?:chatgpt|doubao|jianying)\.com\b/i },
  { label: 'platform upload vendor', pattern: /\b(?:imagex|snssdk)\b/i },
  { label: 'platform internal API path', pattern: /\/backend-api\//i },
  { label: 'platform challenge protocol', pattern: /\b(?:openai-sentinel|estuary)\b/i },
  { label: 'platform asset domain', pattern: /\boaiusercontent\.com\b/i },
]

const SKIP_DIR_NAMES = new Set(['target'])

async function walk(dir) {
  let entries
  try {
    entries = await fs.readdir(dir, { withFileTypes: true })
  } catch {
    return []
  }

  const files = []
  for (const entry of entries) {
    if (SKIP_DIR_NAMES.has(entry.name)) continue
    const absolutePath = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      files.push(...(await walk(absolutePath)))
    } else if (entry.isFile() && entry.name.endsWith('.rs')) {
      files.push(absolutePath)
    }
  }
  return files
}

function findViolations(filePath, content) {
  const violations = []
  // 测试也是架构的一部分，不能借测试文件绕过 Rust 与插件的职责边界。
  const lines = content.split('\n')
  for (const [index, line] of lines.entries()) {
    for (const { label, pattern } of BANNED_PATTERNS) {
      if (pattern.test(line)) {
        const rel = path.relative(REPO_ROOT, filePath).replaceAll(path.sep, '/')
        violations.push(`${rel}:${index + 1} ${label}: ${line.trim()}`)
      }
    }
  }
  return violations
}

async function main() {
  const files = (await Promise.all(RUST_ROOTS.map((root) => walk(root)))).flat()
  const violations = []
  for (const filePath of files) {
    const content = await fs.readFile(filePath, 'utf8')
    violations.push(...findViolations(filePath, content))
  }

  if (violations.length === 0) {
    console.log('Rust platform boundary check passed.')
    return
  }

  console.error('\nRust platform boundary violations:\n')
  for (const violation of violations) {
    console.error(`  ${violation}`)
  }
  console.error('\nMove platform-specific rules to plugin manifests or plugin scripts.')
  process.exitCode = 1
}

await main()

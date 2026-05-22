#!/usr/bin/env node

/**
 * 轻量级重复代码检查。
 *
 * 设计思路：
 * 1. 扫描目标模块代码文件，构建固定窗口的"归一化代码片段"索引。
 * 2. 只在当前改动文件参与重复片段时才报错，避免历史存量问题一次性阻断所有开发。
 * 3. 重点拦截明显的复制粘贴片段，而不是追求学术级检测精度。
 */

import { promises as fs } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const CURRENT_FILE = fileURLToPath(import.meta.url)
const REPO_ROOT = path.resolve(path.dirname(CURRENT_FILE), '..')
const WINDOW_SIZE = 10
const MAX_REPORTS = 12

const SCOPE_ROOTS = {
  app: ['src', 'src-tauri/src', 'plugins'],
}

const SUPPORTED_EXTENSIONS = new Set(['.ts', '.tsx', '.js', '.jsx', '.go', '.rs'])
const SKIP_DIR_NAMES = new Set([
  'node_modules',
  'dist',
  'build',
  'coverage',
  'target',
  '.next',
  'gen',
])

const LOW_SIGNAL_PATTERNS = [
  /^if err != nil \{$/,
  /^return nil, err$/,
  /^return err$/,
  /^return nil$/,
  /^defer .*Close\(\)$/,
  /^req\.Header\.Set\(/,
  /^resp, err := http\.DefaultClient\.Do\(req\)$/,
  /^body, err := io\.ReadAll\(resp\.Body\)$/,
]

function parseArgs(argv) {
  let scope = ''
  const targets = []

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--scope') {
      scope = argv[index + 1] || ''
      index += 1
      continue
    }
    targets.push(argv[index])
  }

  if (!scope || !Object.prototype.hasOwnProperty.call(SCOPE_ROOTS, scope)) {
    throw new Error(`缺少有效 --scope，当前支持: ${Object.keys(SCOPE_ROOTS).join(', ')}`)
  }

  return { scope, targets }
}

async function walkDirectory(rootDir) {
  const entries = await fs.readdir(rootDir, { withFileTypes: true })
  const files = []

  for (const entry of entries) {
    if (SKIP_DIR_NAMES.has(entry.name)) {
      continue
    }

    const absolutePath = path.join(rootDir, entry.name)
    if (entry.isDirectory()) {
      files.push(...(await walkDirectory(absolutePath)))
      continue
    }

    if (entry.isFile() && SUPPORTED_EXTENSIONS.has(path.extname(entry.name))) {
      files.push(absolutePath)
    }
  }

  return files
}

function normalizeLine(line) {
  const trimmed = line.trim()
  if (!trimmed) {
    return ''
  }
  if (
    trimmed.startsWith('//') ||
    trimmed.startsWith('/*') ||
    trimmed.startsWith('*') ||
    trimmed.startsWith('import ') ||
    trimmed.startsWith('export {') ||
    trimmed.startsWith('package ') ||
    trimmed.startsWith('use ') ||
    // Go import 块内部的包路径行
    /^"[^"]+"\)?$/.test(trimmed)
  ) {
    return ''
  }

  if (LOW_SIGNAL_PATTERNS.some((pattern) => pattern.test(trimmed))) {
    return ''
  }

  return trimmed.replace(/\s+/g, ' ')
}

function buildWindows(content) {
  const normalized = content
    .split(/\r?\n/)
    .map((line, index) => ({
      lineNumber: index + 1,
      normalized: normalizeLine(line),
    }))
    .filter((item) => item.normalized)

  const windows = []
  for (let index = 0; index <= normalized.length - WINDOW_SIZE; index += 1) {
    const slice = normalized.slice(index, index + WINDOW_SIZE)
    const joined = slice.map((item) => item.normalized).join('\n')

    // 太短的片段通常只是模板壳或重复 import，不值得拦。
    if (joined.length < 180) {
      continue
    }

    windows.push({
      signature: joined,
      startLine: slice[0].lineNumber,
    })
  }

  return windows
}

function toRelative(filePath) {
  return path.relative(REPO_ROOT, filePath).replaceAll(path.sep, '/')
}

async function collectScopeFiles(scope) {
  const files = []
  for (const relativeRoot of SCOPE_ROOTS[scope]) {
    const absoluteRoot = path.join(REPO_ROOT, relativeRoot)
    try {
      const stat = await fs.stat(absoluteRoot)
      if (!stat.isDirectory()) {
        continue
      }
    } catch {
      continue
    }
    files.push(...(await walkDirectory(absoluteRoot)))
  }
  return [...new Set(files)].sort()
}

async function main() {
  const { scope, targets } = parseArgs(process.argv.slice(2))
  const targetSet = new Set(targets.map((filePath) => path.resolve(filePath)))

  if (targetSet.size === 0) {
    console.log(`${scope} 没有需要执行重复代码检查的改动文件。`)
    return
  }

  const files = await collectScopeFiles(scope)
  const windowsBySignature = new Map()

  for (const filePath of files) {
    const content = await fs.readFile(filePath, 'utf8')
    const windows = buildWindows(content)

    for (const window of windows) {
      const bucket = windowsBySignature.get(window.signature) || []
      bucket.push({
        filePath,
        startLine: window.startLine,
      })
      windowsBySignature.set(window.signature, bucket)
    }
  }

  const reports = []
  for (const matches of windowsBySignature.values()) {
    if (matches.length < 2) {
      continue
    }

    const interesting = matches.filter((item) => targetSet.has(path.resolve(item.filePath)))
    if (interesting.length === 0) {
      continue
    }

    const firstTarget = interesting[0]
    const firstOther = matches.find((item) => item.filePath !== firstTarget.filePath)

    if (!firstOther) {
      continue
    }

    reports.push({
      target: firstTarget,
      other: firstOther,
    })
  }

  const uniqueReports = []
  const seen = new Set()
  for (const report of reports) {
    const key = [
      toRelative(report.target.filePath),
      report.target.startLine,
      toRelative(report.other.filePath),
      report.other.startLine,
    ].join(':')
    if (seen.has(key)) {
      continue
    }
    seen.add(key)
    uniqueReports.push(report)
  }

  if (uniqueReports.length === 0) {
    console.log(`${scope} 重复代码检查通过。`)
    return
  }

  console.error('\n发现疑似复制粘贴代码，已阻断启动：\n')
  for (const report of uniqueReports.slice(0, MAX_REPORTS)) {
    console.error(
      `  ${toRelative(report.target.filePath)}:${report.target.startLine} ` +
        `与 ${toRelative(report.other.filePath)}:${report.other.startLine} 存在重复片段`
    )
  }

  if (uniqueReports.length > MAX_REPORTS) {
    console.error(`  ... 另外还有 ${uniqueReports.length - MAX_REPORTS} 处未展开`)
  }

  console.error(
    '\n建议：把公共逻辑提取到 shared/utils、hooks、services 或通用组件，不要继续复制现有实现。'
  )
  process.exitCode = 1
}

await main()

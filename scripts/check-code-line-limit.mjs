#!/usr/bin/env node

/**
 * 仓库级代码行数守卫。
 *
 * MediaGen 当前代码主要分布在 Web Console、Rust 服务端、通用 crates、插件和脚本中。
 * 失败时直接阻断启动、构建和检查链路，避免单文件继续膨胀。
 */

import { promises as fs } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const CURRENT_FILE = fileURLToPath(import.meta.url)
const REPO_ROOT = path.resolve(path.dirname(CURRENT_FILE), '..')
const DEFAULT_MAX_LINES = 500

const SOURCE_SCOPES = {
  app: [
    'apps/console/src',
    'apps/server/src',
    'apps/local-launcher/src',
    'crates',
    'plugins',
    'scripts',
  ],
}

const SUPPORTED_EXTENSIONS = new Set([
  '.ts',
  '.tsx',
  '.js',
  '.jsx',
  '.mjs',
  '.cjs',
  '.css',
  '.rs',
  '.go',
])

const SKIP_DIR_NAMES = new Set([
  'node_modules',
  'dist',
  'build',
  'coverage',
  'target',
  'vendor',
  '.git',
  '.next',
  'gen',
])

function parseArgs(argv) {
  let scope = 'all'
  let maxLines = DEFAULT_MAX_LINES

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--scope') {
      scope = argv[index + 1] || ''
      index += 1
      continue
    }
    if (arg === '--max') {
      const rawValue = argv[index + 1]
      const parsedValue = Number(rawValue)
      if (!rawValue || Number.isNaN(parsedValue) || parsedValue <= 0) {
        throw new Error('Missing or invalid value for --max')
      }
      maxLines = parsedValue
      index += 1
    }
  }

  if (scope !== 'all' && !Object.prototype.hasOwnProperty.call(SOURCE_SCOPES, scope)) {
    throw new Error(
      `Unsupported scope "${scope}". Expected one of: all, ${Object.keys(SOURCE_SCOPES).join(', ')}`
    )
  }

  return { scope, maxLines }
}

function resolveScopeRoots(scope) {
  if (scope === 'all') {
    return Object.values(SOURCE_SCOPES).flat()
  }
  return SOURCE_SCOPES[scope]
}

async function walkDirectory(rootDir) {
  const entries = await fs.readdir(rootDir, { withFileTypes: true })
  const files = []

  for (const entry of entries) {
    if (SKIP_DIR_NAMES.has(entry.name)) continue

    const absolutePath = path.join(rootDir, entry.name)
    if (entry.isDirectory()) {
      files.push(...(await walkDirectory(absolutePath)))
      continue
    }

    if (!entry.isFile()) continue
    if (!SUPPORTED_EXTENSIONS.has(path.extname(entry.name))) continue

    files.push(absolutePath)
  }

  return files
}

async function collectSourceFiles(scope) {
  const roots = resolveScopeRoots(scope)
  const files = []

  for (const relativeRoot of roots) {
    const absoluteRoot = path.join(REPO_ROOT, relativeRoot)
    try {
      const stat = await fs.stat(absoluteRoot)
      if (!stat.isDirectory()) continue
    } catch {
      continue
    }

    files.push(...(await walkDirectory(absoluteRoot)))
  }

  return [...new Set(files)].sort()
}

async function countLines(filePath) {
  const content = await fs.readFile(filePath, 'utf8')
  return content.split(/\r?\n/).length
}

function toRelativePath(filePath) {
  return path.relative(REPO_ROOT, filePath).replaceAll(path.sep, '/')
}

async function main() {
  const { scope, maxLines } = parseArgs(process.argv.slice(2))
  const files = await collectSourceFiles(scope)

  const offenders = []
  for (const filePath of files) {
    const lineCount = await countLines(filePath)
    if (lineCount > maxLines) {
      offenders.push({ filePath: toRelativePath(filePath), lineCount })
    }
  }

  offenders.sort((left, right) => right.lineCount - left.lineCount)

  if (offenders.length === 0) {
    console.log(`✅ ${scope} 代码行数检查通过，所有代码文件都在 ${maxLines} 行以内。`)
    return
  }

  console.error(`\n❌ ${scope} 存在超过 ${maxLines} 行的代码文件，已阻断当前启动/构建/检查流程：\n`)
  for (const offender of offenders) {
    console.error(`  ${offender.lineCount.toString().padStart(4, ' ')}  ${offender.filePath}`)
  }

  console.error('\n注意：禁止通过放宽阈值或继续往同一文件里堆逻辑来规避这个限制。')
  console.error(
    `这些文件不能再继续改大；超过 ${maxLines} 行时，请按职责边界拆分页面、组件、service、helper 或领域模块。`
  )

  process.exitCode = 1
}

await main()

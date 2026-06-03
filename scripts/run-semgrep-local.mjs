#!/usr/bin/env node

/**
 * Run Semgrep locally for startup and manual quality scans.
 *
 * Strategy:
 * 1. 优先使用本机 semgrep CLI，避免把质量门禁绑死在 Docker 上。
 * 2. 如果本机 Homebrew Python / semgrep 运行环境损坏，使用 uvx 执行同一套规则。
 * 3. 启动门禁使用 include 白名单，只扫描当前目标文件，避免假通过。
 */

import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const CURRENT_FILE = fileURLToPath(import.meta.url)
const REPO_ROOT = path.resolve(path.dirname(CURRENT_FILE), '..')
const DEFAULT_CONFIG = 'quality/semgrep/mediagen-start.yml'

function parseArgs(argv) {
  let config = DEFAULT_CONFIG
  const targets = []

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--config') {
      config = argv[index + 1] || DEFAULT_CONFIG
      index += 1
      continue
    }
    targets.push(arg)
  }

  return { config, targets }
}

function commandExists(command) {
  const result = spawnSync('bash', ['-lc', `command -v ${command}`], {
    cwd: REPO_ROOT,
    stdio: 'ignore',
  })
  return result.status === 0
}

function normalizeTarget(target) {
  const absoluteTarget = path.resolve(REPO_ROOT, target)
  return path.relative(REPO_ROOT, absoluteTarget).replaceAll(path.sep, '/')
}

function buildSemgrepArgs(config, targets) {
  const relativeTargets = targets.map(normalizeTarget)

  return [
    'scan',
    '--config',
    config,
    '--error',
    '--severity',
    'ERROR',
    '.',
    ...relativeTargets.flatMap((target) => ['--include', target]),
  ]
}

function runSemgrepCommand(command, args) {
  const result = spawnSync(command, args, {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  })

  if (result.stdout) process.stdout.write(result.stdout)
  if (result.stderr) process.stderr.write(result.stderr)
  return result
}

function isSemgrepRuntimeBroken(result) {
  const output = `${result.error ? result.error.message : ''}\n${result.stderr || ''}`
  return (
    Boolean(result.error) ||
    output.includes('Library not loaded') ||
    output.includes('library load denied by system policy') ||
    output.includes('code signature') ||
    output.includes('ImportError: dlopen')
  )
}

function runLocalSemgrep(config, targets) {
  return runSemgrepCommand('semgrep', buildSemgrepArgs(config, targets))
}

function runUvxSemgrep(config, targets) {
  return runSemgrepCommand('uvx', ['semgrep', ...buildSemgrepArgs(config, targets)])
}

function main() {
  const { config, targets } = parseArgs(process.argv.slice(2))

  if (targets.length === 0) {
    console.log('[semgrep] 当前没有需要扫描的目标文件，跳过。')
    return
  }

  let result
  if (commandExists('semgrep')) {
    console.log('[semgrep] 使用本地 semgrep CLI 扫描。')
    result = runLocalSemgrep(config, targets)
    if (result.status !== 0 && isSemgrepRuntimeBroken(result) && commandExists('uvx')) {
      console.log('[semgrep] 本地 semgrep 运行环境异常，改用 uvx semgrep 执行同一套规则。')
      result = runUvxSemgrep(config, targets)
    }
  } else if (commandExists('uvx')) {
    console.log('[semgrep] 未找到本地 semgrep CLI，改用 uvx semgrep 执行同一套规则。')
    result = runUvxSemgrep(config, targets)
  } else {
    throw new Error('未找到 semgrep CLI 或 uvx。请先安装：brew install semgrep 或 brew install uv')
  }

  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

try {
  main()
} catch (error) {
  console.error(`[semgrep][error] ${error instanceof Error ? error.message : String(error)}`)
  process.exitCode = 1
}

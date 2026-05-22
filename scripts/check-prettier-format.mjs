#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import { existsSync, statSync } from 'node:fs'
import path from 'node:path'

const REPO_ROOT = process.cwd()
const SUPPORTED_EXTENSIONS = new Set([
  '.ts',
  '.tsx',
  '.js',
  '.jsx',
  '.mjs',
  '.cjs',
  '.json',
  '.css',
  '.scss',
  '.md',
  '.yml',
  '.yaml',
])
const SKIP_SEGMENTS = new Set([
  'node_modules',
  'dist',
  'build',
  'coverage',
  'target',
  'vendor',
  '.next',
  'out',
  'gen',
])

function parseArgs(argv) {
  const explicitFiles = []
  let checkAll = false

  for (const arg of argv) {
    if (arg === '--all') {
      checkAll = true
      continue
    }
    explicitFiles.push(arg)
  }

  return { checkAll, explicitFiles }
}

function runGitCommand(args) {
  const result = spawnSync('git', args, { cwd: REPO_ROOT, encoding: 'utf8' })
  if (result.status !== 0) {
    throw new Error(result.stderr?.trim() || `git ${args.join(' ')} 执行失败`)
  }
  return result.stdout
    .split('\n')
    .map((item) => item.trim())
    .filter(Boolean)
}

function collectChangedFilesFromGit() {
  return [
    ...runGitCommand(['diff', '--name-only', '--diff-filter=ACMRTUXB', 'HEAD']),
    ...runGitCommand(['ls-files', '--others', '--exclude-standard']),
  ]
}

function collectAllFilesFromGit() {
  return [
    ...runGitCommand(['ls-files']),
    ...runGitCommand(['ls-files', '--others', '--exclude-standard']),
  ]
}

function isIgnoredFile(relativePath) {
  const normalized = relativePath.replaceAll('\\', '/')
  const segments = normalized.split('/')
  return segments.some((segment) => SKIP_SEGMENTS.has(segment))
}

function collectCheckableFiles(rawPaths) {
  return [...new Set(rawPaths)]
    .filter((relativePath) => {
      if (!relativePath || isIgnoredFile(relativePath)) return false
      const absolutePath = path.resolve(REPO_ROOT, relativePath)
      if (!existsSync(absolutePath) || !statSync(absolutePath).isFile()) return false
      return SUPPORTED_EXTENSIONS.has(path.extname(relativePath))
    })
    .sort()
}

function runPrettierCheck(files, { checkAll }) {
  if (files.length === 0) {
    console.log(
      checkAll
        ? '✅ 当前整个仓库没有需要执行 Prettier 校验的文件。'
        : '✅ 当前没有需要执行 Prettier 校验的改动文件。'
    )
    return
  }

  const result = spawnSync('npx', ['prettier', '--check', ...files], {
    cwd: REPO_ROOT,
    stdio: 'inherit',
  })

  if (typeof result.status === 'number') {
    process.exitCode = result.status
    return
  }

  process.exitCode = 1
}

const { checkAll, explicitFiles } = parseArgs(process.argv.slice(2))
const files = collectCheckableFiles(
  explicitFiles.length > 0
    ? explicitFiles
    : checkAll
      ? collectAllFilesFromGit()
      : collectChangedFilesFromGit()
)

runPrettierCheck(files, { checkAll })

#!/usr/bin/env node

/**
 * 开发启动前质量门禁。
 *
 * 统一入口，避免每个模块各写一套脚本。
 * 规则分两层：
 * 1. 基础门禁：lint / typecheck / go vet 等现有硬规则。
 * 2. 增量门禁：只对当前改动文件执行 Semgrep / 复杂度 / 重复代码检查，优先约束新增质量问题。
 */

import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const CURRENT_FILE = fileURLToPath(import.meta.url)
const REPO_ROOT = path.resolve(path.dirname(CURRENT_FILE), '..', '..')
const SUPPORTED_MODULES = ['app']
const CODE_EXTENSIONS = new Set(['.ts', '.tsx', '.js', '.jsx', '.go', '.rs'])
const CHANGED_FILES_ENV_KEY = 'MEDIAGEN_CHANGED_FILES_JSON'
const MODULE_PREFIXES = {
  app: ['src/', 'src-tauri/', 'plugins/', 'scripts/'],
}

function parseArgs(argv) {
  let modules = []
  let includeCoverage = process.env.MEDIAGEN_INCLUDE_COVERAGE_GATE === '1'

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--include-coverage') {
      includeCoverage = true
      continue
    }
    if (arg === '--modules') {
      modules = String(argv[index + 1] || '')
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean)
      index += 1
    }
  }

  if (modules.length === 0) {
    modules = SUPPORTED_MODULES
  }

  const invalid = modules.filter((item) => !SUPPORTED_MODULES.includes(item))
  if (invalid.length > 0) {
    throw new Error(`不支持的模块: ${invalid.join(', ')}`)
  }

  return { modules, includeCoverage }
}

function stripAnsi(text) {
  return text.replace(/\u001B\[[0-9;]*m/g, '')
}

// 构建过程中的噪音行模式，匹配后在成功输出中过滤掉，避免污染 AI 上下文
const BUILD_NOISE_PATTERNS = [
  /^\s*(Compiling|Downloading|Downloaded|Blocking waiting|Updating|Unpacking|Fetching)\s+/,
  /^\s*Finished\s+(dev|release|test|bench)\s/,
  /^\s*Running\s+`/,
  /^\s*Fresh\s+\S+\s+v/,
  /^\s*Locking\s+\d+\s+package/,
  /^\s*Adding\s+\S+\s+v/,
]

function filterBuildNoise(text) {
  if (!text) return text
  return text
    .split(/\r?\n/)
    .filter((line) => !BUILD_NOISE_PATTERNS.some((pat) => pat.test(line)))
    .join('\n')
}

function extractFailureHighlights(output) {
  const lines = stripAnsi(output)
    .split(/\r?\n/)
    .map((line) => line.trimEnd())
    .filter(Boolean)

  const interestingPatterns = [
    /error TS\d+:/i,
    /\berror\b/i,
    /\bfailed\b/i,
    /\bcannot\b/i,
    /\bundefined\b/i,
    /\bnot found\b/i,
    /\bproblems?\b/i,
    /:\d+(?::\d+)?\s+/,
    /✖/,
  ]

  const highlights = lines.filter((line) =>
    interestingPatterns.some((pattern) => pattern.test(line))
  )

  if (highlights.length > 0) {
    return highlights.slice(0, 8)
  }

  return lines.slice(-8)
}

function buildFixPrinciples(label) {
  if (label.includes('lint')) {
    return [
      '先删未使用的导入、变量和参数，再处理真正的类型或规范问题。',
      '发现重复实现时优先抽公共方法，不要为了过 lint 再复制一份新逻辑。',
      '禁止通过关闭 ESLint / golangci-lint 规则、加忽略注释来绕过门禁。',
    ]
  }

  if (label.includes('typecheck') || label.includes('type-check')) {
    return [
      '先统一「定义方」和「调用方」的类型契约，不要只在调用处临时强转。',
      '优先修 null / undefined、泛型、第三方组件桥接这类真实类型边界问题。',
      '除非确实在做边界适配，否则不要直接用 any 压过去；确实要桥接时要写清楚注释。',
    ]
  }

  if (label.includes('vet')) {
    return [
      '把 vet 当成潜在 bug 提示来修，不要把报错当成「风格问题」。',
      '优先处理空分支、可疑赋值、不可达逻辑、格式化参数不匹配这类高风险项。',
      '禁止用空日志、空判断或吞错方式让门禁静默通过。',
    ]
  }

  if (label.includes('Semgrep')) {
    return [
      '先按规则提示修真实架构边界、安全边界或返回体问题，不要先想怎么跳过规则。',
      '如果是项目规范要求的分层限制，应该把逻辑移回正确层，而不是继续在原地补丁。',
      '只有在规则与项目规范冲突时，才调整规则口径；不能为了省事把规则整体放开。',
    ]
  }

  if (label.includes('行数')) {
    return [
      '按职责边界拆分文件：页面、组件、service、helper 或领域模块各自独立。',
      '禁止通过放宽阈值或继续往同一文件里堆逻辑来规避限制。',
      '拆分后要保持行为不变，确保编译和测试通过。',
    ]
  }

  if (label.includes('复杂度')) {
    return [
      '把长函数拆成更小的步骤函数，让每个函数只负责一个清晰动作。',
      '多分支判断优先下沉到 helper / service / adapter，不要继续往当前函数里补 if/else。',
      '拆分后要保持行为不变，再补验证，不要为了拆分顺手改业务语义。',
    ]
  }

  if (label.includes('重复代码')) {
    return [
      '把重复片段提取到 shared/utils、hooks、services 或通用组件，不要复制现有实现。',
      '如果只是名字不同、流程相同，优先抽参数化 helper，而不是继续平铺多个版本。',
      '抽取公共逻辑后要让调用方语义更清晰，而不是再套一层更难懂的封装。',
    ]
  }

  if (label.includes('覆盖率')) {
    return [
      '新增或修改的 service/lib 层逻辑必须补对应单元测试，不能只写实现不写测试。',
      '测试要覆盖核心分支和边界情况，不要只写 happy path。',
      '禁止通过降低覆盖率阈值或跳过门禁来绕过检查。',
    ]
  }

  if (label.includes('Prettier') || label.includes('格式')) {
    return [
      '运行 npx prettier --write 修复格式问题，不要手动调整空格和换行。',
      '检查 .prettierrc 配置是否与项目规范一致。',
      '禁止通过 prettier-ignore 注释绕过格式检查。',
    ]
  }

  return [
    '先看门禁摘录里的第一条具体报错，优先修最靠前的根因问题。',
    '不要通过跳过脚本、放宽阈值、关闭规则来绕过门禁。',
    '修完后请重新跑同一条门禁，确认不是只修掉了表面报错。',
  ]
}

function printFailureHelp(label, output) {
  const highlights = extractFailureHighlights(output)
  const principles = buildFixPrinciples(label)

  console.error(`\n[start-quality] ${label} 失败摘录：`)
  for (const line of highlights) {
    console.error(`  - ${line}`)
  }

  console.error('\n[start-quality] 修改原则：')
  for (const principle of principles) {
    console.error(`  - ${principle}`)
  }
}

function runCommand(label, command, args, options = {}) {
  console.log(`\n[start-quality] ${label}`)
  const result = spawnSync(command, args, {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    shell: false,
    env: process.env,
    ...options,
  })

  const stdout = result.stdout || ''
  const stderr = result.stderr || ''

  const spawnError = result.error ? `${result.error.name}: ${result.error.message}` : ''
  if (result.error || result.status !== 0) {
    if (stdout) {
      process.stdout.write(stdout)
    }
    if (stderr) {
      process.stderr.write(stderr)
    }
    if (spawnError) {
      process.stderr.write(`${spawnError}\n`)
    }
    printFailureHelp(label, `${stdout}\n${stderr}\n${spawnError}`)
    throw new Error(`${label} 未通过`)
  }

  // 成功时过滤构建噪音，避免编译进度等无关信息污染 AI 上下文
  const filteredStdout = filterBuildNoise(stdout)
  const filteredStderr = filterBuildNoise(stderr)
  if (filteredStdout.trim()) process.stdout.write(filteredStdout)
  if (filteredStderr.trim()) process.stderr.write(filteredStderr)
}

function shellCommandOutput(command) {
  const result = spawnSync('bash', ['-lc', command], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  })

  if (result.status !== 0) {
    return ''
  }

  return (result.stdout || '').trim()
}

function gitOutput(args) {
  const result = spawnSync('git', args, {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  })
  if (result.status !== 0) {
    return ''
  }
  return (result.stdout || '').trim()
}

function collectChangedFiles() {
  const changed = new Set()

  const tracked = gitOutput(['diff', '--name-only', '--diff-filter=ACMR', 'HEAD', '--'])
  const untracked = gitOutput(['ls-files', '--others', '--exclude-standard'])

  for (const output of [tracked, untracked]) {
    for (const line of output.split('\n')) {
      const relativePath = line.trim()
      if (!relativePath) {
        continue
      }
      changed.add(relativePath.replaceAll(path.sep, '/'))
    }
  }

  return [...changed]
}

export function resolveInjectedChangedFiles(rawValue = process.env[CHANGED_FILES_ENV_KEY]) {
  if (!rawValue) {
    return null
  }

  let parsed
  try {
    parsed = JSON.parse(rawValue)
  } catch {
    throw new Error(`${CHANGED_FILES_ENV_KEY} 不是合法 JSON 数组`)
  }

  if (!Array.isArray(parsed)) {
    throw new Error(`${CHANGED_FILES_ENV_KEY} 必须是字符串数组`)
  }

  return [
    ...new Set(
      parsed
        .map((item) => String(item || '').trim())
        .filter(Boolean)
        .map((item) => item.replaceAll('\\', '/').replaceAll(path.sep, '/'))
    ),
  ]
}

export function resolveChangedFiles() {
  const injectedFiles = resolveInjectedChangedFiles()
  if (injectedFiles) {
    console.log(
      `[start-quality] 使用上层传入的改动文件 ${injectedFiles.length} 个，跳过仓库全量 diff 推断。`
    )
    return injectedFiles
  }

  return collectChangedFiles()
}

export function filterChangedCodeFiles(moduleName, files) {
  const existingChangedFiles = files.filter((filePath) => {
    const absolutePath = path.join(REPO_ROOT, filePath)
    return fs.existsSync(absolutePath)
  })

  const skippedDeletedFiles = files.length - existingChangedFiles.length
  if (skippedDeletedFiles > 0) {
    console.log(
      `[start-quality] ${moduleName} 跳过 ${skippedDeletedFiles} 个已删除改动文件，避免增量检查误读不存在路径。`
    )
  }

  return existingChangedFiles
    .filter((filePath) => MODULE_PREFIXES[moduleName].some((prefix) => filePath.startsWith(prefix)))
    .filter((filePath) => CODE_EXTENSIONS.has(path.extname(filePath)))
    .map((filePath) => path.join(REPO_ROOT, filePath))
}

function runFoundationChecks(moduleName) {
  switch (moduleName) {
    case 'app':
      runCommand('app lint', 'npm', ['run', 'lint'])
      runCommand('app type-check', 'npm', ['run', 'type-check'])
      runCommand('app cargo-check', 'npm', ['run', 'cargo-check'])
      break
    default:
      break
  }
}

function runDeltaChecks(moduleName, changedFiles) {
  if (changedFiles.length === 0) {
    console.log(`[start-quality] ${moduleName} 当前没有改动代码文件，跳过增量检查。`)
    return
  }

  runCommand(`${moduleName} Semgrep 自定义规则检查`, 'node', [
    'scripts/run-semgrep-local.mjs',
    ...changedFiles,
  ])
  runCommand(`${moduleName} 复杂度检查`, 'node', [
    'scripts/gates/check-structural-complexity.mjs',
    ...changedFiles,
  ])
  runCommand(`${moduleName} 重复代码检查`, 'node', [
    'scripts/check-duplicate-code.mjs',
    '--scope',
    moduleName,
    ...changedFiles,
  ])
}

function runCoverageChecks(moduleName) {
  switch (moduleName) {
    case 'app':
      runCommand('app 覆盖率检查', 'node', ['scripts/gates/check-coverage-gate.mjs', '--app'])
      break
    default:
      break
  }
}

async function main() {
  if (process.env.MEDIAGEN_SKIP_START_QUALITY_GATE === '1') {
    console.log('[start-quality] 已通过上层入口执行过质量门禁，当前启动跳过重复检查。')
    return
  }

  const { modules, includeCoverage } = parseArgs(process.argv.slice(2))
  const changedFiles = resolveChangedFiles()

  console.log(`[start-quality] 准备检查模块: ${modules.join(', ')}`)
  if (includeCoverage) {
    console.log('[start-quality] 已启用覆盖率门禁。')
  }

  for (const moduleName of modules) {
    runFoundationChecks(moduleName)
    runDeltaChecks(moduleName, filterChangedCodeFiles(moduleName, changedFiles))
    if (includeCoverage) {
      runCoverageChecks(moduleName)
    }
  }

  console.log('\n[start-quality] 启动前质量门禁通过。')
}

if (CURRENT_FILE === path.resolve(process.argv[1] || '')) {
  try {
    await main()
  } catch (error) {
    console.error(
      `\n[start-quality][error] ${error instanceof Error ? error.message : String(error)}`
    )
    process.exitCode = 1
  }
}

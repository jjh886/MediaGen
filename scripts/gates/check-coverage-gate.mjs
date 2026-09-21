#!/usr/bin/env node

/**
 * 测试覆盖率门禁。
 *
 * app：在 Web Console 内执行 vitest 覆盖率，在 Rust workspace 内执行 cargo-llvm-cov。
 *
 * 用法：
 *   node scripts/gates/check-coverage-gate.mjs         # app UI + Rust
 *   node scripts/gates/check-coverage-gate.mjs --app   # app UI + Rust
 *
 * 覆盖率门禁必须真实执行，不提供跳过入口。
 */

import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync, rmSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const CURRENT_FILE = fileURLToPath(import.meta.url)
const REPO_ROOT = path.resolve(path.dirname(CURRENT_FILE), '..', '..')
const APP_DIR = path.join(REPO_ROOT, 'apps', 'console')
const RUST_COVERAGE_JSON_PATH = path.join(REPO_ROOT, 'target', 'llvm-cov-summary.json')

// ─── 阈值配置 ───
const APP_FRONTEND_THRESHOLD = 75
const RUST_THRESHOLD = 75

// ─── App UI 覆盖范围：业务核心层（services / hooks / utils / stores） ───
const APP_FRONTEND_INCLUDE = [
  'src/features/*/services/**/*.ts',
  'src/features/*/lib/**/*.ts',
  'src/features/*/hooks/**/*.ts',
  'src/shared/services/**/*.ts',
  'src/shared/utils/**/*.ts',
  'src/shared/stores/**/*.ts',
  'src/shared/lib/**/*.ts',
  'src/shared/hooks/**/*.ts',
]

const FRONTEND_EXCLUDE = ['**/*.test.ts', '**/*.test.tsx', '**/*.spec.ts', '**/*.spec.tsx']

// ─── Rust 覆盖范围：可稳定单元测试的核心模块 ───
// runtime.rs 会启动外部浏览器进程并探测 CDP 端口，放到集成验证里看，不纳入单元覆盖率汇总。
const RUST_INCLUDE_REGEX = [
  'apps/server/src/access_control\\.rs$',
  'apps/server/src/account_browser_profile\\.rs$',
  'apps/server/src/admin_routes\\.rs$',
  'apps/server/src/image_request\\.rs$',
  'apps/server/src/net_client\\.rs$',
  'apps/server/src/(protocol_.*|model_api|audio_api|realtime_api|realtime_transport|request_identity|sse_decoder|plugin_file_io|plugin_http_.*|plugin_locks|plugin_process|audio_transcode|store_accounts|account_status_refresh|account_login_credentials)\\.rs$',
  'crates/mediagen-plugin-runtime/src/js_runtime\\.rs$',
  'apps/server/src/store\\.rs$',
  'apps/server/src/store_entities\\.rs$',
  'apps/server/src/store_persistence\\.rs$',
  'apps/server/src/flow_runtime\\.rs$',
  'apps/server/src/value_utils\\.rs$',
  'crates/mediagen-browser-gateway/src/fingerprint\\.rs$',
  'crates/mediagen-browser-gateway/src/types\\.rs$',
  'crates/mediagen-core/src/plugin_manifest\\.rs$',
  'crates/mediagen-plugin-runtime/src/plugin_loader\\.rs$',
].join('|')

function parseArgs(argv) {
  const args = argv.slice(2)
  const defaultTargets = {
    appFrontend: true,
    rust: true,
  }

  if (args.length === 0) return defaultTargets
  const includesApp = args.includes('--app') || args.includes('--all')

  return {
    appFrontend: includesApp || args.includes('--ui'),
    rust: includesApp || args.includes('--rust'),
  }
}

function checkThreshold(label, linesPct, branchesPct, functionsPct, threshold) {
  console.log('')
  console.log(`[coverage-gate] ${label} 覆盖率结果:`)
  console.log(`  行覆盖率:   ${linesPct}%`)
  console.log(`  分支覆盖率: ${branchesPct}%`)
  console.log(`  函数覆盖率: ${functionsPct}%`)

  const failures = []
  if (linesPct < threshold) failures.push(`行覆盖率 ${linesPct}% < ${threshold}%`)
  if (branchesPct < threshold) failures.push(`分支覆盖率 ${branchesPct}% < ${threshold}%`)
  if (functionsPct < threshold) failures.push(`函数覆盖率 ${functionsPct}% < ${threshold}%`)

  if (failures.length > 0) {
    console.error('')
    console.error(`[coverage-gate] ${label} 覆盖率未达标:`)
    for (const f of failures) console.error(`  - ${f}`)
    console.error('')
    console.error('[coverage-gate] 修改原则:')
    console.error(
      '  - 新增或修改的 service/lib/engine 层逻辑必须补对应单元测试，不能只写实现不写测试。'
    )
    console.error('  - 测试要覆盖核心分支和边界情况，不要只写 happy path。')
    console.error('  - 禁止通过降低覆盖率阈值或跳过门禁来绕过检查。')
    return false
  }

  console.log(`[coverage-gate] ${label} 覆盖率门禁通过 (阈值 ${threshold}%)`)
  return true
}

function readCoverageSummary(summaryPath) {
  if (!existsSync(summaryPath)) {
    console.error(`[coverage-gate] 未找到覆盖率报告: ${summaryPath}`)
    return null
  }

  try {
    const summary = JSON.parse(readFileSync(summaryPath, 'utf8'))
    if (!summary.total) {
      console.error('[coverage-gate] 覆盖率报告中没有 total 字段')
      return null
    }
    return summary.total
  } catch {
    console.error('[coverage-gate] 无法解析覆盖率报告')
    return null
  }
}

function coverageReportDir(projectDir, name) {
  return path.join(projectDir, 'coverage', `.gate-${name}-${process.pid}-${Date.now()}`)
}

function runVitestCoverage(cwd, args, coverageDir, errorMessage) {
  try {
    const result = spawnSync('npx', args, {
      cwd,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      env: process.env,
    })

    if (result.stdout) process.stdout.write(result.stdout)
    if (result.stderr) process.stderr.write(result.stderr)

    if (result.error || result.status !== 0) {
      console.error(errorMessage)
      return null
    }

    return readCoverageSummary(path.join(coverageDir, 'coverage-summary.json'))
  } finally {
    rmSync(coverageDir, { recursive: true, force: true })
  }
}

function parseLlvmCovJson(output) {
  const start = output.indexOf('{"data"')
  if (start === -1) return null

  let depth = 0
  let inString = false
  let escaped = false

  for (let i = start; i < output.length; i += 1) {
    const char = output[i]

    if (inString) {
      if (escaped) {
        escaped = false
      } else if (char === '\\') {
        escaped = true
      } else if (char === '"') {
        inString = false
      }
      continue
    }

    if (char === '"') {
      inString = true
    } else if (char === '{') {
      depth += 1
    } else if (char === '}') {
      depth -= 1
      if (depth === 0) {
        try {
          return JSON.parse(output.slice(start, i + 1))
        } catch {
          return null
        }
      }
    }
  }

  return null
}

function runAppFrontendCoverage() {
  const coverageDir = coverageReportDir(APP_DIR, 'console')

  const args = [
    'vitest',
    'run',
    '--coverage',
    '--coverage.provider=istanbul',
    '--coverage.reporter=json-summary',
    `--coverage.reportsDirectory=${coverageDir}`,
    ...APP_FRONTEND_INCLUDE.map((p) => `--coverage.include=${p}`),
    ...FRONTEND_EXCLUDE.map((p) => `--coverage.exclude=${p}`),
  ]

  console.log(`[coverage-gate] App UI 覆盖率检查，阈值 ${APP_FRONTEND_THRESHOLD}%`)

  const total = runVitestCoverage(
    APP_DIR,
    args,
    coverageDir,
    '[coverage-gate] vitest --coverage 执行失败'
  )
  if (!total) {
    return false
  }

  return checkThreshold(
    'App UI',
    total.lines?.pct ?? 0,
    total.branches?.pct ?? 0,
    total.functions?.pct ?? 0,
    APP_FRONTEND_THRESHOLD
  )
}

function runRustCoverage() {
  // 检查 cargo-llvm-cov 是否可用
  const checkResult = spawnSync('cargo', ['llvm-cov', '--version'], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  })

  if (checkResult.status !== 0) {
    console.error('[coverage-gate] cargo-llvm-cov 未安装，无法计算 Rust 覆盖率。')
    console.error('[coverage-gate] 安装方式: cargo install cargo-llvm-cov')
    return false
  }

  console.log(`[coverage-gate] Rust 覆盖率检查，阈值 ${RUST_THRESHOLD}%`)
  rmSync(RUST_COVERAGE_JSON_PATH, { force: true })

  const result = spawnSync(
    'cargo',
    [
      'llvm-cov',
      '--workspace',
      '--json',
      '--summary-only',
      '--output-path',
      RUST_COVERAGE_JSON_PATH,
    ],
    {
      cwd: REPO_ROOT,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      env: process.env,
      maxBuffer: 1024 * 1024 * 20,
    }
  )

  if (result.stderr) {
    const filtered = result.stderr
      .split('\n')
      .filter((l) => !/^\s*(Compiling|Downloading|Finished|Running|Fresh|Locking)\s/.test(l))
      .join('\n')
      .trim()
    if (filtered) process.stderr.write(filtered + '\n')
  }

  let report = null
  if (existsSync(RUST_COVERAGE_JSON_PATH)) {
    try {
      report = JSON.parse(readFileSync(RUST_COVERAGE_JSON_PATH, 'utf8'))
    } catch {
      report = null
    }
  }
  if (!report) {
    report = parseLlvmCovJson(result.stdout)
  }

  if (!report) {
    console.error('[coverage-gate] 无法解析 Rust 覆盖率报告')
    if (result.error || result.status !== 0) {
      console.error('[coverage-gate] cargo llvm-cov 执行失败')
      if (result.stdout) {
        const output =
          result.stdout.length > 4000
            ? `...省略前面的测试输出...\n${result.stdout.slice(-4000)}`
            : result.stdout
        process.stdout.write(output)
      }
    }
    return false
  }

  if (result.error || result.status !== 0) {
    console.error('[coverage-gate] cargo llvm-cov 编译、测试或报告生成失败。')
    return false
  }

  const files = report?.data?.[0]?.files ?? []
  const includeRegex = new RegExp(RUST_INCLUDE_REGEX)

  // 手动过滤文件并汇总覆盖率
  let totalLines = 0,
    coveredLines = 0
  let totalFunctions = 0,
    coveredFunctions = 0
  let totalBranches = 0,
    coveredBranches = 0

  for (const f of files) {
    if (!includeRegex.test(f.filename)) continue
    const s = f.summary
    totalLines += s.lines?.count ?? 0
    coveredLines += s.lines?.covered ?? 0
    totalFunctions += s.functions?.count ?? 0
    coveredFunctions += s.functions?.covered ?? 0
    totalBranches += s.branches?.count ?? 0
    coveredBranches += s.branches?.covered ?? 0
  }

  const linesPct = totalLines > 0 ? +((coveredLines / totalLines) * 100).toFixed(1) : 0
  const functionsPct =
    totalFunctions > 0 ? +((coveredFunctions / totalFunctions) * 100).toFixed(1) : 0
  // Rust llvm-cov 的 branches 数据通常为 0，用 lines + functions 双维度即可
  const branchesPct =
    totalBranches > 0 ? +((coveredBranches / totalBranches) * 100).toFixed(1) : linesPct

  return checkThreshold('Rust', linesPct, branchesPct, functionsPct, RUST_THRESHOLD)
}

function main() {
  if (process.env.MEDIAGEN_SKIP_COVERAGE_GATE === '1') {
    console.error('[coverage-gate] 禁止通过 MEDIAGEN_SKIP_COVERAGE_GATE 跳过覆盖率门禁。')
    process.exitCode = 1
    return
  }

  const { appFrontend, rust } = parseArgs(process.argv)
  let allPassed = true

  if (appFrontend) {
    if (!runAppFrontendCoverage()) allPassed = false
  }

  if (rust) {
    if (!runRustCoverage()) allPassed = false
  }

  if (!allPassed) {
    process.exitCode = 1
  }
}

main()

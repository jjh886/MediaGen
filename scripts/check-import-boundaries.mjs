#!/usr/bin/env node

/**
 * 前端分层导入边界检查。
 *
 * 扫描 apps/console/src 三层架构：
 *   app -> features -> shared
 * shared 不能反向依赖 features / app，features 不能反向依赖 app，
 * 同一层级下 feature 之间不得互相 import。
 */

import { promises as fs } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const CURRENT_FILE = fileURLToPath(import.meta.url)
const REPO_ROOT = path.resolve(path.dirname(CURRENT_FILE), '..')
const SRC_ROOTS = [
  {
    label: 'console',
    root: path.join(REPO_ROOT, 'apps', 'console', 'src'),
    checkCrossFeature: true,
  },
]
const IMPORT_PATTERN = /from\s+['"]([^'"]+)['"]/g

const SUPPORTED_EXTENSIONS = new Set(['.ts', '.tsx', '.js', '.jsx'])
const SKIP_DIR_NAMES = new Set(['node_modules', 'dist', 'build', 'coverage', '.git'])

function layerOf(filePath) {
  const normalizedPath = filePath.replaceAll(path.sep, '/')
  if (normalizedPath.includes('/src/app/')) return 'app'
  if (normalizedPath.includes('/src/features/')) return 'features'
  if (normalizedPath.includes('/src/shared/')) return 'shared'
  return 'other'
}

function resolveImport(sourceFile, specifier, srcRoot) {
  if (specifier.startsWith('@/')) {
    return path.join(srcRoot, specifier.slice(2))
  }
  if (specifier.startsWith('.')) {
    return path.resolve(path.dirname(sourceFile), specifier)
  }
  return null
}

function violatesBoundary(fromLayer, toLayer) {
  if (fromLayer === 'shared' && (toLayer === 'features' || toLayer === 'app')) return true
  if (fromLayer === 'features' && toLayer === 'app') return true
  return false
}

function featureOf(filePath) {
  const match = filePath.replaceAll(path.sep, '/').match(/\/features\/([^/]+)\//)
  return match ? match[1] : null
}

// 跨 feature 导入豁免名单：
// - '@/features/auth/store' 仅在 C4 迁移期作为 shared/stores/auth 的 re-export shim 存在，
//   COMPAT(2026-10-24) 到期删除后名单可一并清理
const CROSS_FEATURE_EXEMPT_SPECIFIERS = new Set(['@/features/auth/store'])

async function walkDirectory(rootDir) {
  let entries
  try {
    entries = await fs.readdir(rootDir, { withFileTypes: true })
  } catch {
    return []
  }

  const files = []
  for (const entry of entries) {
    if (SKIP_DIR_NAMES.has(entry.name)) continue
    const absolutePath = path.join(rootDir, entry.name)
    if (entry.isDirectory()) {
      files.push(...(await walkDirectory(absolutePath)))
    } else if (entry.isFile() && SUPPORTED_EXTENSIONS.has(path.extname(entry.name))) {
      files.push(absolutePath)
    }
  }
  return files
}

async function scanRoot(srcRoot, checkCrossFeature) {
  const files = await walkDirectory(srcRoot)
  const violations = []

  for (const filePath of files) {
    const fromLayer = layerOf(filePath)
    if (fromLayer === 'other') continue

    const content = await fs.readFile(filePath, 'utf8')
    let match
    IMPORT_PATTERN.lastIndex = 0

    while ((match = IMPORT_PATTERN.exec(content)) !== null) {
      const specifier = match[1]
      const resolved = resolveImport(filePath, specifier, srcRoot)
      if (!resolved) continue

      const toLayer = layerOf(resolved)
      if (toLayer === 'other') continue

      if (violatesBoundary(fromLayer, toLayer)) {
        const rel = path.relative(REPO_ROOT, filePath).replaceAll(path.sep, '/')
        violations.push(`${rel}: ${fromLayer} → ${toLayer} (${specifier})`)
      }

      // 跨 feature 导入检查（豁免名单内的 specifier 跳过）
      if (checkCrossFeature && fromLayer === 'features' && toLayer === 'features') {
        const fromFeature = featureOf(filePath)
        const toFeature = featureOf(resolved)
        if (
          fromFeature &&
          toFeature &&
          fromFeature !== toFeature &&
          !CROSS_FEATURE_EXEMPT_SPECIFIERS.has(specifier)
        ) {
          const rel = path.relative(REPO_ROOT, filePath).replaceAll(path.sep, '/')
          violations.push(`${rel}: feature/${fromFeature} → feature/${toFeature} (${specifier})`)
        }
      }
    }
  }

  return violations
}

async function main() {
  const allViolations = []
  for (const { label, root, checkCrossFeature } of SRC_ROOTS) {
    try {
      await fs.access(root)
    } catch {
      // 某些环境下可能缺失其中一个前端目录，跳过即可
      continue
    }
    const violations = await scanRoot(root, checkCrossFeature)
    for (const v of violations) allViolations.push(`[${label}] ${v}`)
  }

  if (allViolations.length === 0) {
    console.log('✅ 前端导入边界检查通过。')
    return
  }

  console.error('\n❌ 发现导入边界违规：\n')
  for (const v of allViolations) {
    console.error(`  ${v}`)
  }
  console.error('\n规则：app → features → shared，shared 不能反向依赖，features 之间不能互相导入。')

  process.exitCode = 1
}

await main()

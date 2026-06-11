import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const defaultDesktopDir = path.resolve(scriptDir, '..')

const RELATIVE_ARTIFACT_DIRS = [
  'dist',
  path.join('src-tauri', 'target', 'debug'),
  path.join('src-tauri', 'target', 'llvm-cov-target'),
  path.join('src-tauri', 'target', 'release', 'build'),
  path.join('src-tauri', 'target', 'release', 'deps'),
  path.join('src-tauri', 'target', 'release', 'incremental'),
  path.join('src-tauri', 'target', 'release', '.fingerprint'),
  path.join('src-tauri', 'target', 'release', 'examples'),
  path.join('src-tauri', 'target', 'release', 'native'),
  path.join('src-tauri', 'target', 'release', 'plugins'),
]

export async function cleanupTauriBuildArtifacts({ desktopDir = defaultDesktopDir } = {}) {
  const root = path.resolve(desktopDir)
  const result = { removed: [], skipped: [] }

  for (const relativePath of RELATIVE_ARTIFACT_DIRS) {
    const target = path.resolve(root, relativePath)
    assertInside(root, target)

    if (!(await pathExists(target))) {
      result.skipped.push(target)
      continue
    }

    await fs.rm(target, { recursive: true, force: true })
    result.removed.push(target)
  }

  return result
}

async function pathExists(target) {
  try {
    await fs.access(target)
    return true
  } catch {
    return false
  }
}

function assertInside(root, target) {
  const relative = path.relative(root, target)
  if (relative === '' || relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new Error(`拒绝清理 desktop 目录外的路径: ${target}`)
  }
}

function formatSummary(result) {
  const lines = [`已清理 ${result.removed.length} 个 Tauri 非交付构建产物。`]
  for (const item of result.removed) {
    lines.push(`- ${path.relative(defaultDesktopDir, item) || item}`)
  }
  return lines.join('\n')
}

if (import.meta.url === `file://${process.argv[1]}`) {
  cleanupTauriBuildArtifacts()
    .then((result) => {
      console.log(formatSummary(result))
    })
    .catch((error) => {
      console.error(error instanceof Error ? error.message : String(error))
      process.exitCode = 1
    })
}

#!/usr/bin/env node

import { execFileSync, spawnSync } from 'node:child_process'
import { existsSync, readdirSync, rmSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(scriptDir, '..')
const macosBundleDir = path.join(repoRoot, 'src-tauri', 'target', 'release', 'bundle', 'macos')
const tempDmgPattern = /^rw\..+\.dmg$/
const maxBuildAttempts = 2
const cleanupOnly = process.argv.includes('--cleanup-only')

function log(message) {
  console.log(`[tauri-build] ${message}`)
}

function isProjectTempDmg(imagePath) {
  const absolutePath = path.resolve(imagePath)
  const fileName = path.basename(absolutePath)

  return path.dirname(absolutePath) === macosBundleDir && tempDmgPattern.test(fileName)
}

function pickDiskDevice(image) {
  const entities = Array.isArray(image['system-entities']) ? image['system-entities'] : []

  // hdiutil detach 用整块磁盘更稳定，比如 /dev/disk12，而不是 /dev/disk12s1。
  const wholeDisk = entities.find((entity) => /^\/dev\/disk\d+$/.test(entity['dev-entry'] ?? ''))
  return wholeDisk?.['dev-entry'] ?? entities.find((entity) => entity['dev-entry'])?.['dev-entry']
}

function readMountedTempDmgs() {
  const plist = execFileSync('hdiutil', ['info', '-plist'], { encoding: 'utf8' })
  const json = execFileSync('plutil', ['-convert', 'json', '-o', '-', '-'], {
    input: plist,
    encoding: 'utf8',
  })
  const info = JSON.parse(json)

  return (info.images ?? [])
    .filter(
      (image) => typeof image['image-path'] === 'string' && isProjectTempDmg(image['image-path'])
    )
    .map((image) => ({
      device: pickDiskDevice(image),
      imagePath: image['image-path'],
    }))
}

function detachTempDmg({ device, imagePath }) {
  if (!device) {
    log(`跳过无法识别设备号的临时镜像：${imagePath}`)
    return false
  }

  log(`卸载残留 DMG 挂载：${device} (${path.basename(imagePath)})`)
  const normalDetach = spawnSync('hdiutil', ['detach', device], { stdio: 'inherit' })
  if (normalDetach.status === 0) {
    return true
  }

  // Finder 或 Spotlight 偶尔会短暂占用临时盘，普通卸载失败时再强制卸载一次。
  log(`普通卸载失败，尝试强制卸载：${device}`)
  const forceDetach = spawnSync('hdiutil', ['detach', '-force', device], { stdio: 'inherit' })
  return forceDetach.status === 0
}

function deleteStaleTempDmgs() {
  if (!existsSync(macosBundleDir)) {
    return 0
  }

  let deletedCount = 0
  for (const entry of readdirSync(macosBundleDir, { withFileTypes: true })) {
    if (!entry.isFile() || !tempDmgPattern.test(entry.name)) {
      continue
    }

    const filePath = path.join(macosBundleDir, entry.name)
    rmSync(filePath, { force: true })
    deletedCount += 1
  }

  return deletedCount
}

function cleanupTempDmgArtifacts() {
  if (process.platform !== 'darwin') {
    return { detachedCount: 0, deletedCount: 0 }
  }

  let detachedCount = 0
  for (const tempDmg of readMountedTempDmgs()) {
    if (detachTempDmg(tempDmg)) {
      detachedCount += 1
    }
  }

  const deletedCount = deleteStaleTempDmgs()
  if (detachedCount > 0 || deletedCount > 0) {
    log(`清理完成：卸载 ${detachedCount} 个挂载，删除 ${deletedCount} 个临时 DMG 文件`)
  }

  return { detachedCount, deletedCount }
}

function resolveTauriCommand() {
  const executableName = process.platform === 'win32' ? 'tauri.cmd' : 'tauri'
  const localCommand = path.join(repoRoot, 'node_modules', '.bin', executableName)

  return existsSync(localCommand) ? localCommand : executableName
}

function runTauriBuild(attempt) {
  const retryLabel = attempt > 1 ? '（自动重试）' : ''
  log(`开始执行 tauri build${retryLabel}`)

  return spawnSync(resolveTauriCommand(), ['build'], {
    cwd: repoRoot,
    env: process.env,
    stdio: 'inherit',
  })
}

if (cleanupOnly) {
  cleanupTempDmgArtifacts()
  process.exit(0)
}

for (let attempt = 1; attempt <= maxBuildAttempts; attempt += 1) {
  cleanupTempDmgArtifacts()

  const result = runTauriBuild(attempt)
  if (result.status === 0) {
    process.exit(0)
  }

  const exitCode = result.status ?? 1
  if (attempt === maxBuildAttempts) {
    process.exit(exitCode)
  }

  const cleanupResult = cleanupTempDmgArtifacts()
  if (cleanupResult.detachedCount === 0 && cleanupResult.deletedCount === 0) {
    process.exit(exitCode)
  }

  log('检测到失败后残留的 DMG 临时产物，已清理并自动重试一次')
}

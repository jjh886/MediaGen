#!/usr/bin/env node

import { existsSync, lstatSync, mkdtempSync, readdirSync, readlinkSync, rmSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const releaseDir = path.join(root, 'dist-local', 'MediaGen-local')
const installersDir = path.join(root, 'dist-local', 'installers')
const platformName = `${process.platform}-${process.arch}`

const requiredReleaseFiles = [
  'console/index.html',
  'plugins/plugins.index.json',
  process.platform === 'win32' ? 'bin/mediagen-server.exe' : 'bin/mediagen-server',
  process.platform === 'win32' ? 'MediaGen.exe' : 'MediaGen',
]

for (const item of requiredReleaseFiles) {
  assertExists(path.join(releaseDir, item), `发行目录缺少 ${item}`)
}

if (process.platform === 'darwin') {
  const app = path.join(installersDir, 'MediaGen.app')
  const dmg = path.join(installersDir, `MediaGen-${platformName}.dmg`)
  assertExists(app, 'macOS 缺少 MediaGen.app')
  assertExists(path.join(app, 'Contents/MacOS/MediaGen'), 'macOS App 缺少启动器')
  assertExists(
    path.join(app, 'Contents/Resources/MediaGen-local/bin/mediagen-server'),
    'macOS App 缺少服务程序'
  )
  assertExists(dmg, 'macOS 缺少 DMG 安装包')
  assertMacDmgDragInstallLayout(dmg)
}

if (process.platform === 'win32') {
  assertExists(path.join(releaseDir, 'start-mediagen.ps1'), 'Windows 缺少启动脚本')
  assertExists(
    path.join(installersDir, `MediaGen-local-${platformName}.zip`),
    'Windows 缺少 zip 安装包'
  )
  assertExists(
    path.join(installersDir, `MediaGen-${platformName}-Setup.exe`),
    'Windows 缺少 Setup.exe 安装包'
  )
}

if (process.platform === 'linux') {
  assertExists(
    path.join(installersDir, `mediagen_0.1.0_${linuxDebArch()}.deb`),
    'Linux 缺少 deb 安装包'
  )
  assertAnyFile(installersDir, '.rpm', 'Linux 缺少 rpm 安装包')
  assertExists(path.join(releaseDir, 'start-mediagen.sh'), 'Linux 缺少启动脚本')
  assertExists(path.join(releaseDir, 'MediaGen.desktop'), 'Linux 缺少桌面入口')
}

assertExists(
  path.join(installersDir, `MediaGen-local-${platformName}.tar.gz`),
  '缺少通用 tar.gz 归档'
)

console.log('[installer-layout] 本地安装包结构检查通过')

function assertExists(file, message) {
  if (!existsSync(file)) {
    console.error(`[installer-layout] ${message}: ${file}`)
    process.exit(1)
  }
}

function assertAnyFile(dir, suffix, message) {
  const found = readdirSync(dir).some((item) => item.endsWith(suffix))
  if (!found) {
    console.error(`[installer-layout] ${message}: ${dir}`)
    process.exit(1)
  }
}

function assertMacDmgDragInstallLayout(dmg) {
  const mountPoint = mkdtempSync(path.join(os.tmpdir(), 'mediagen-dmg-layout-'))
  let attached = false
  let failureMessage = ''

  try {
    const attach = spawnSync(
      'hdiutil',
      ['attach', '-nobrowse', '-readonly', '-mountpoint', mountPoint, dmg],
      { encoding: 'utf8' }
    )
    if (attach.status !== 0) {
      throw new Error(`macOS DMG 挂载失败: ${attach.stderr || attach.stdout}`)
    }
    attached = true

    assertMountedPath(path.join(mountPoint, 'MediaGen.app'), 'macOS DMG 缺少 MediaGen.app')
    assertMountedPath(path.join(mountPoint, '.DS_Store'), 'macOS DMG 缺少 Finder 布局文件')
    // 这里验证拖拽安装入口：Finder 会把这个链接显示成 Applications 文件夹。
    const applications = path.join(mountPoint, 'Applications')
    assertMountedPath(applications, 'macOS DMG 缺少 Applications 快捷方式')
    if (!lstatSync(applications).isSymbolicLink()) {
      throw new Error(`macOS DMG 的 Applications 不是快捷方式: ${applications}`)
    }
    if (readlinkSync(applications) !== '/Applications') {
      throw new Error(`macOS DMG 的 Applications 没有指向 /Applications: ${applications}`)
    }
  } catch (error) {
    failureMessage = error instanceof Error ? error.message : String(error)
  } finally {
    if (attached) {
      spawnSync('hdiutil', ['detach', mountPoint], { stdio: 'ignore' })
    }
    rmSync(mountPoint, { recursive: true, force: true })
  }

  if (failureMessage) fail(failureMessage)
}

function assertMountedPath(file, message) {
  if (!existsSync(file)) throw new Error(`${message}: ${file}`)
}

function linuxDebArch() {
  if (process.arch === 'x64') return 'amd64'
  if (process.arch === 'arm64') return 'arm64'
  if (process.arch === 'ia32') return 'i386'
  return process.arch
}

function fail(message) {
  console.error(`[installer-layout] ${message}`)
  process.exit(1)
}

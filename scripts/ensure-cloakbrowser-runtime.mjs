#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { createWriteStream, existsSync } from 'node:fs'
import { access, cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { pipeline } from 'node:stream/promises'
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const outputDir =
  optionValue('--output') || path.join(root, 'dist-local', 'MediaGen-local', 'browser-runtime')
const cacheRoot = optionValue('--cache') || path.join(root, 'dist-local', 'cloakbrowser-cache')
const repoApi = 'https://api.github.com/repos/CloakHQ/CloakBrowser/releases?per_page=100'

const platformTag = currentPlatformTag()
const result = await ensureFromGithub()
console.log(`已打包 CloakBrowser 免费版：${result.version} / ${result.source}`)
console.log(`运行时目录：${outputDir}`)

async function ensureFromGithub() {
  const release = await latestFreeRelease(platformTag)
  const asset = release.assets.find((item) => item.name === assetName(platformTag))
  if (!asset) throw new Error(`CloakBrowser ${platformTag} 免费版资源不存在`)

  const workDir = path.join(cacheRoot, platformTag, release.tag_name)
  const archivePath = path.join(workDir, asset.name)
  const extractDir = path.join(workDir, 'extract')

  await mkdir(workDir, { recursive: true })
  await downloadIfNeeded(asset.browser_download_url, archivePath)
  await verifySha256(release, asset.name, archivePath)
  await rm(extractDir, { recursive: true, force: true })
  await mkdir(extractDir, { recursive: true })
  await extractArchive(archivePath, extractDir)
  await replaceRuntime(runtimeRoot(extractDir), {
    source: 'github',
    repository: 'CloakHQ/CloakBrowser',
    release: release.tag_name,
    asset: asset.name,
    platform: platformTag,
  })
  return { source: `github:${asset.name}`, version: release.tag_name }
}

async function replaceRuntime(from, metadata) {
  await rm(outputDir, { recursive: true, force: true })
  await mkdir(outputDir, { recursive: true })
  await cp(from, outputDir, { recursive: true })
  await chmodRuntime(outputDir)
  await writeFile(
    path.join(outputDir, 'mediagen-cloakbrowser.json'),
    JSON.stringify({ ...metadata, downloaded_at: new Date().toISOString() }, null, 2) + '\n'
  )
}

async function latestFreeRelease(tag) {
  const response = await fetch(repoApi, {
    headers: {
      Accept: 'application/vnd.github+json',
      'User-Agent': 'MediaGen-Packager',
    },
  })
  if (!response.ok) throw new Error(`读取 CloakBrowser Releases 失败: HTTP ${response.status}`)
  const releases = await response.json()
  const found = releases.find(
    (item) =>
      !item.draft &&
      !item.prerelease &&
      !String(item.tag_name).includes('-pro') &&
      item.assets?.some((asset) => asset.name === assetName(tag))
  )
  if (!found) throw new Error(`没有找到 ${tag} 的 CloakBrowser 免费版 release`)
  return found
}

async function downloadIfNeeded(url, target) {
  if ((await exists(target)) && !process.env.MEDIAGEN_FORCE_CLOAKBROWSER_DOWNLOAD) return
  console.log(`下载 CloakBrowser：${url}`)
  const response = await fetch(url, {
    headers: {
      Accept: 'application/octet-stream',
      'User-Agent': 'MediaGen-Packager',
    },
  })
  if (!response.ok) throw new Error(`下载失败: HTTP ${response.status}`)
  await pipeline(response.body, createWriteStream(target))
}

async function verifySha256(release, name, filePath) {
  const sums = release.assets.find((asset) => asset.name === 'SHA256SUMS')
  if (!sums) return
  const response = await fetch(sums.browser_download_url, {
    headers: { 'User-Agent': 'MediaGen-Packager' },
  })
  if (!response.ok) throw new Error(`下载 SHA256SUMS 失败: HTTP ${response.status}`)
  const text = await response.text()
  const line = text
    .split(/\r?\n/)
    .map((item) => item.trim())
    .find((item) => item.endsWith(` ${name}`) || item.endsWith(` *${name}`))
  if (!line) throw new Error(`SHA256SUMS 中没有 ${name}`)
  const expected = line.split(/\s+/)[0]
  const actual = createHash('sha256')
    .update(await readFile(filePath))
    .digest('hex')
  if (actual !== expected) throw new Error(`CloakBrowser 校验失败: ${actual} != ${expected}`)
}

async function extractArchive(archive, target) {
  if (archive.endsWith('.zip')) {
    if (process.platform === 'win32') {
      await run('powershell', [
        '-NoProfile',
        '-Command',
        `Expand-Archive -Force -Path '${escapePs(archive)}' -DestinationPath '${escapePs(target)}'`,
      ])
    } else {
      await run('unzip', ['-q', archive, '-d', target])
    }
    return
  }
  await run('tar', ['-xzf', archive, '-C', target])
}

function runtimeRoot(extractDir) {
  const chromiumApp = path.join(extractDir, 'Chromium.app')
  if (pathExistsSync(chromiumApp)) return extractDir
  const linuxBrowserBinary = path.join(extractDir, 'chrome')
  if (pathExistsSync(linuxBrowserBinary)) return extractDir
  const windowsBrowserBinary = path.join(extractDir, 'chrome.exe')
  if (pathExistsSync(windowsBrowserBinary)) return extractDir
  for (const name of ['cloakbrowser', 'chrome-linux', 'chrome-win', 'chromium']) {
    const candidate = path.join(extractDir, name)
    if (pathExistsSync(candidate)) return candidate
  }
  throw new Error(`无法识别 CloakBrowser 解压目录：${extractDir}`)
}

async function chmodRuntime(dir) {
  const candidates = [
    path.join(dir, 'chrome'),
    path.join(dir, 'chrome.exe'),
    path.join(dir, 'Chromium.app', 'Contents', 'MacOS', 'Chromium'),
  ]
  for (const candidate of candidates) {
    if (await exists(candidate)) await run('chmod', ['755', candidate]).catch(() => undefined)
  }
}

function currentPlatformTag() {
  if (process.platform === 'darwin') return process.arch === 'arm64' ? 'darwin-arm64' : 'darwin-x64'
  if (process.platform === 'linux') return process.arch === 'arm64' ? 'linux-arm64' : 'linux-x64'
  if (process.platform === 'win32') return 'windows-x64'
  throw new Error(`不支持的打包平台：${process.platform}-${process.arch}`)
}

function assetName(tag) {
  return `cloakbrowser-${tag}.${tag.startsWith('windows') ? 'zip' : 'tar.gz'}`
}

async function exists(filePath) {
  try {
    await access(filePath)
    return true
  } catch {
    return false
  }
}

function pathExistsSync(filePath) {
  return existsSync(filePath)
}

function optionValue(name) {
  const index = process.argv.indexOf(name)
  if (index < 0) return ''
  return process.argv[index + 1] || ''
}

function escapePs(value) {
  return value.replaceAll("'", "''")
}

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: root,
      stdio: 'inherit',
      shell: process.platform === 'win32',
    })
    child.on('exit', (code) => {
      if (code === 0) resolve()
      else reject(new Error(`${command} ${args.join(' ')} failed with ${code}`))
    })
    child.on('error', reject)
  })
}

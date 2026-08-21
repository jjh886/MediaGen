#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { createReadStream, createWriteStream, existsSync } from 'node:fs'
import {
  access,
  cp,
  mkdir,
  readdir,
  readFile,
  readlink,
  rm,
  symlink,
  writeFile,
} from 'node:fs/promises'
import { pipeline } from 'node:stream/promises'
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const outputDir =
  optionValue('--output') || path.join(root, 'dist-local', 'MediaGen-local', 'browser-runtime')
const cacheRoot = optionValue('--cache') || path.join(root, 'dist-local', 'cloakbrowser-cache')
const repoApi = 'https://api.github.com/repos/CloakHQ/CloakBrowser/releases?per_page=100'
const cacheManifestName = 'cache-manifest.json'
const fetchAttempts = 4

const platformTag = currentPlatformTag()
const result = await ensureRuntime()
console.log(`已打包 CloakBrowser 免费版：${result.version} / ${result.source}`)
console.log(`运行时目录：${outputDir}`)

async function ensureRuntime() {
  if (!process.env.MEDIAGEN_FORCE_CLOAKBROWSER_DOWNLOAD) {
    const cached = await ensureFromVerifiedCache()
    if (cached) return cached
  }
  try {
    return await ensureFromGithub()
  } catch (error) {
    console.warn(`GitHub 暂时不可用，尝试正式缓存：${errorMessage(error)}`)
    const cached = await ensureFromVerifiedCache()
    if (cached) return cached
    throw error
  }
}

async function ensureFromGithub() {
  const release = await latestFreeRelease(platformTag)
  const asset = release.assets.find((item) => item.name === assetName(platformTag))
  if (!asset) throw new Error(`CloakBrowser ${platformTag} 免费版资源不存在`)

  const workDir = path.join(cacheRoot, platformTag, release.tag_name)
  const archivePath = path.join(workDir, asset.name)
  const extractDir = path.join(workDir, 'extract')

  await mkdir(workDir, { recursive: true })
  await downloadIfNeeded(asset.browser_download_url, archivePath)
  const sha256 = await verifySha256(release, asset.name, archivePath)
  await writeCacheManifest(workDir, {
    repository: 'CloakHQ/CloakBrowser',
    release: release.tag_name,
    asset: asset.name,
    platform: platformTag,
    sha256,
  })
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

async function ensureFromVerifiedCache() {
  const platformCache = path.join(cacheRoot, platformTag)
  const releases = await directoryNames(platformCache)
  releases.sort((a, b) => b.localeCompare(a, undefined, { numeric: true }))
  for (const release of releases) {
    const workDir = path.join(platformCache, release)
    const manifest = await readJson(path.join(workDir, cacheManifestName))
    if (!validCacheManifest(manifest, release)) continue
    const archivePath = path.join(workDir, manifest.asset)
    if (!(await exists(archivePath))) continue
    if ((await sha256File(archivePath)) !== manifest.sha256) continue

    const extractDir = path.join(workDir, 'extract')
    await rm(extractDir, { recursive: true, force: true })
    await mkdir(extractDir, { recursive: true })
    await extractArchive(archivePath, extractDir)
    await replaceRuntime(runtimeRoot(extractDir), {
      source: 'verified-cache',
      repository: manifest.repository,
      release: manifest.release,
      asset: manifest.asset,
      platform: manifest.platform,
      sha256: manifest.sha256,
    })
    console.log(`复用已校验的 CloakBrowser 缓存：${manifest.release}`)
    return { source: `cache:${manifest.asset}`, version: manifest.release }
  }
  return null
}

async function replaceRuntime(from, metadata) {
  await rm(outputDir, { recursive: true, force: true })
  await mkdir(outputDir, { recursive: true })
  await cp(from, outputDir, { recursive: true, verbatimSymlinks: true })
  await normalizeMacFrameworkSymlinks(outputDir)
  await chmodRuntime(outputDir)
  await writeFile(
    path.join(outputDir, 'mediagen-cloakbrowser.json'),
    JSON.stringify({ ...metadata, downloaded_at: new Date().toISOString() }, null, 2) + '\n'
  )
}

async function normalizeMacFrameworkSymlinks(runtimeDir) {
  if (process.platform !== 'darwin') return
  const framework = path.join(
    runtimeDir,
    'Chromium.app/Contents/Frameworks/Chromium Framework.framework'
  )
  const currentVersion = await symlinkBasename(path.join(framework, 'Versions/Current'))
  if (!currentVersion) return

  // CloakBrowser 的 macOS 发行包可能携带构建机绝对链接；改成 App 内标准相对链接后才能搬运和签名。
  await replaceSymlink(path.join(framework, 'Versions/Current'), currentVersion)
  for (const name of ['Resources', 'Libraries', 'Chromium Framework', 'Helpers']) {
    await replaceSymlink(path.join(framework, name), `Versions/Current/${name}`)
  }
}

async function symlinkBasename(filePath) {
  try {
    return path.basename(await readlink(filePath))
  } catch (error) {
    if (error.code === 'ENOENT' || error.code === 'EINVAL') return ''
    throw error
  }
}

async function replaceSymlink(filePath, target) {
  await rm(filePath, { force: true })
  await symlink(target, filePath)
}

async function latestFreeRelease(tag) {
  const response = await fetchWithRetry(
    repoApi,
    {
      headers: {
        Accept: 'application/vnd.github+json',
        'User-Agent': 'MediaGen-Packager',
      },
    },
    '读取 CloakBrowser Releases'
  )
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
  const response = await fetchWithRetry(
    url,
    {
      headers: {
        Accept: 'application/octet-stream',
        'User-Agent': 'MediaGen-Packager',
      },
    },
    '下载 CloakBrowser'
  )
  if (!response.ok) throw new Error(`下载失败: HTTP ${response.status}`)
  await pipeline(response.body, createWriteStream(target))
}

async function verifySha256(release, name, filePath) {
  const sums = release.assets.find((asset) => asset.name === 'SHA256SUMS')
  if (!sums) throw new Error('CloakBrowser Release 缺少 SHA256SUMS，拒绝缓存未校验文件')
  const response = await fetchWithRetry(
    sums.browser_download_url,
    {
      headers: { 'User-Agent': 'MediaGen-Packager' },
    },
    '下载 CloakBrowser SHA256SUMS'
  )
  if (!response.ok) throw new Error(`下载 SHA256SUMS 失败: HTTP ${response.status}`)
  const text = await response.text()
  const line = text
    .split(/\r?\n/)
    .map((item) => item.trim())
    .find((item) => item.endsWith(` ${name}`) || item.endsWith(` *${name}`))
  if (!line) throw new Error(`SHA256SUMS 中没有 ${name}`)
  const expected = line.split(/\s+/)[0]
  const actual = await sha256File(filePath)
  if (actual !== expected) throw new Error(`CloakBrowser 校验失败: ${actual} != ${expected}`)
  return actual
}

async function writeCacheManifest(workDir, manifest) {
  await writeFile(
    path.join(workDir, cacheManifestName),
    JSON.stringify({ ...manifest, verified_at: new Date().toISOString() }, null, 2) + '\n',
    'utf8'
  )
}

async function sha256File(filePath) {
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(filePath)) hash.update(chunk)
  return hash.digest('hex')
}

async function directoryNames(dir) {
  try {
    return (await readdir(dir, { withFileTypes: true }))
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name)
  } catch (error) {
    if (error.code === 'ENOENT') return []
    throw error
  }
}

async function readJson(filePath) {
  try {
    return JSON.parse(await readFile(filePath, 'utf8'))
  } catch (error) {
    if (error.code === 'ENOENT' || error instanceof SyntaxError) return null
    throw error
  }
}

function validCacheManifest(manifest, release) {
  return (
    manifest?.release === release &&
    manifest?.platform === platformTag &&
    manifest?.asset === assetName(platformTag) &&
    typeof manifest?.sha256 === 'string' &&
    /^[a-f0-9]{64}$/.test(manifest.sha256)
  )
}

async function fetchWithRetry(url, options, label) {
  let lastError
  for (let attempt = 1; attempt <= fetchAttempts; attempt += 1) {
    try {
      const response = await fetch(url, options)
      if (response.status < 500 && response.status !== 429) return response
      await response.body?.cancel()
      throw new Error(`HTTP ${response.status}`)
    } catch (error) {
      lastError = error
      if (attempt === fetchAttempts) break
      console.warn(`${label}失败，第 ${attempt}/${fetchAttempts} 次重试：${errorMessage(error)}`)
      await new Promise((resolve) => setTimeout(resolve, attempt * 1000))
    }
  }
  throw new Error(`${label}失败：${errorMessage(lastError)}`, { cause: lastError })
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error)
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

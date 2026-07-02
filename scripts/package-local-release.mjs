import { copyFile, cp, mkdir, rm, writeFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const dist = path.join(root, 'dist-local', 'MediaGen-local')

await run('npm', ['--workspace', '@mediagen/console', 'run', 'build'])
await run('cargo', ['build', '--release', '-p', 'mediagen-server', '-p', 'mediagen-local-launcher'])

await rm(dist, { recursive: true, force: true })
await mkdir(path.join(dist, 'bin'), { recursive: true })
await mkdir(path.join(dist, 'console'), { recursive: true })
await mkdir(path.join(dist, 'icons'), { recursive: true })

await cp(path.join(root, 'apps/console/dist'), path.join(dist, 'console'), { recursive: true })
await cp(path.join(root, 'plugins'), path.join(dist, 'plugins'), { recursive: true })
await cp(path.join(root, 'scripts/start'), dist, { recursive: true })
await copyDirIfExists(path.join(root, 'deploy/local'), path.join(dist, 'docs'))
await bundleCloakBrowserRuntime()

await copyIfExists(
  path.join(root, 'target/release/mediagen-server'),
  path.join(dist, 'bin/mediagen-server')
)
await copyIfExists(
  path.join(root, 'target/release/mediagen-server.exe'),
  path.join(dist, 'bin/mediagen-server.exe')
)
await copyIfExists(
  path.join(root, 'target/release/mediagen-local-launcher'),
  path.join(dist, 'MediaGen')
)
await copyIfExists(
  path.join(root, 'target/release/mediagen-local-launcher.exe'),
  path.join(dist, 'MediaGen.exe')
)
await copyIfExists(
  path.join(root, 'apps/local-launcher/assets/mediagen.ico'),
  path.join(dist, 'icons/mediagen.ico')
)
await copyIfExists(
  path.join(root, 'apps/local-launcher/assets/mediagen.icns'),
  path.join(dist, 'icons/mediagen.icns')
)
await copyIfExists(
  path.join(root, 'apps/local-launcher/assets/mediagen.png'),
  path.join(dist, 'icons/mediagen.png')
)
await writeLinuxDesktopFile()

console.log(`本机发行包已生成：${dist}`)

async function copyIfExists(from, to) {
  try {
    await copyFile(from, to)
  } catch (error) {
    if (error.code !== 'ENOENT') throw error
  }
}

async function copyDirIfExists(from, to) {
  try {
    await cp(from, to, { recursive: true })
  } catch (error) {
    if (error.code !== 'ENOENT') throw error
  }
}

async function bundleCloakBrowserRuntime() {
  const source = process.env.MEDIAGEN_CLOAK_BROWSER_DIR || process.env.CLOAK_BROWSER_BUNDLE_DIR
  if (source) {
    await cp(source, path.join(dist, 'browser-runtime'), { recursive: true })
    ensureRuntimeExecutable(path.join(dist, 'browser-runtime'))
    return
  }
  await run('node', [
    'scripts/ensure-cloakbrowser-runtime.mjs',
    '--output',
    path.join(dist, 'browser-runtime'),
  ])
  if (!existsSync(path.join(dist, 'browser-runtime'))) {
    throw new Error('CloakBrowser 运行时没有写入发行包')
  }
  ensureRuntimeExecutable(path.join(dist, 'browser-runtime'))
}

function ensureRuntimeExecutable(runtimeDir) {
  const candidates = [
    path.join(runtimeDir, 'chrome'),
    path.join(runtimeDir, 'chrome.exe'),
    path.join(runtimeDir, 'Chromium.app', 'Contents', 'MacOS', 'Chromium'),
    path.join(runtimeDir, 'CloakBrowser.app', 'Contents', 'MacOS', 'CloakBrowser'),
  ]
  if (!candidates.some((candidate) => existsSync(candidate))) {
    throw new Error(`CloakBrowser 运行时缺少可执行文件：${runtimeDir}`)
  }
}

async function writeLinuxDesktopFile() {
  const desktopFile = [
    '[Desktop Entry]',
    'Type=Application',
    'Name=MediaGen',
    'Comment=Start MediaGen Web Console',
    `Exec=${path.join(dist, 'start-mediagen.sh')}`,
    `Icon=${path.join(dist, 'icons/mediagen.png')}`,
    'Terminal=false',
    'Categories=Graphics;Utility;',
    '',
  ].join('\n')
  await mkdir(dist, { recursive: true })
  await writeFile(path.join(dist, 'MediaGen.desktop'), desktopFile, 'utf8')
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

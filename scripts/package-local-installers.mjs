import { chmod, copyFile, cp, mkdir, rm, writeFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { spawn, spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const releaseDir = path.join(root, 'dist-local', 'MediaGen-local')
const installersDir = path.join(root, 'dist-local', 'installers')
const platformName = `${process.platform}-${process.arch}`
const version = '0.1.0'

await run('node', ['scripts/package-local-release.mjs'])
await rm(installersDir, { recursive: true, force: true })
await mkdir(installersDir, { recursive: true })

if (process.platform === 'darwin') {
  const appPath = await createMacApp()
  await createDmg(appPath)
}

if (process.platform === 'win32') {
  await createWindowsInstaller()
}

if (process.platform === 'linux') {
  await createDebIfPossible()
  await createRpmIfPossible()
}

await createArchive()
console.log(`安装包产物已生成：${installersDir}`)

async function createMacApp() {
  const appPath = path.join(installersDir, 'MediaGen.app')
  const contents = path.join(appPath, 'Contents')
  const macos = path.join(contents, 'MacOS')
  const resources = path.join(contents, 'Resources')
  const resourcePayload = path.join(resources, 'MediaGen-local')

  await mkdir(macos, { recursive: true })
  await mkdir(resources, { recursive: true })
  await cp(releaseDir, resourcePayload, { recursive: true })
  await copyFile(path.join(releaseDir, 'MediaGen'), path.join(macos, 'MediaGen'))
  await chmod(path.join(macos, 'MediaGen'), 0o755)
  await copyIfExists(
    path.join(releaseDir, 'icons', 'mediagen.icns'),
    path.join(resources, 'mediagen.icns')
  )
  await writeFile(path.join(contents, 'Info.plist'), macInfoPlist(), 'utf8')
  return appPath
}

async function createDmg(appPath) {
  if (!commandAvailable('hdiutil')) throw new Error('缺少 hdiutil，无法生成 macOS DMG')
  await run('hdiutil', [
    'create',
    '-volname',
    'MediaGen',
    '-srcfolder',
    appPath,
    '-ov',
    '-format',
    'UDZO',
    path.join(installersDir, `MediaGen-${platformName}.dmg`),
  ])
}

async function createWindowsZip(
  zipPath = path.join(installersDir, `MediaGen-local-${platformName}.zip`)
) {
  if (!commandAvailable('powershell')) {
    throw new Error('缺少 PowerShell，无法生成 Windows 安装 payload')
  }
  await run('powershell', [
    '-NoProfile',
    '-Command',
    `Compress-Archive -Force -Path '${escapePs(releaseDir)}' -DestinationPath '${escapePs(zipPath)}'`,
  ])
}

async function createWindowsInstaller() {
  if (!commandAvailable('iexpress')) {
    throw new Error('缺少 IExpress，无法生成 Windows 安装包')
  }
  const staging = path.join(installersDir, 'windows-installer')
  const payloadZip = path.join(staging, `MediaGen-local-${platformName}.zip`)
  await rm(staging, { recursive: true, force: true })
  await mkdir(staging, { recursive: true })
  await createWindowsZip(payloadZip)
  await writeFile(path.join(staging, 'install-windows.cmd'), windowsInstallCmd(), 'utf8')
  await writeFile(path.join(staging, 'install-windows.ps1'), windowsInstallPs1(payloadZip), 'utf8')
  const sedPath = path.join(staging, 'mediagen-installer.sed')
  const exePath = path.join(installersDir, `MediaGen-${platformName}-Setup.exe`)
  await writeFile(sedPath, iexpressSed(staging, payloadZip, exePath), 'utf8')
  await run('iexpress', ['/N', sedPath])
}

async function createArchive() {
  await run('tar', [
    '-czf',
    path.join(installersDir, `MediaGen-local-${platformName}.tar.gz`),
    '-C',
    path.dirname(releaseDir),
    path.basename(releaseDir),
  ])
}

async function createDebIfPossible() {
  if (!commandAvailable('dpkg-deb')) throw new Error('缺少 dpkg-deb，无法生成 apt/deb 安装包')
  const debRoot = path.join(installersDir, 'deb-root')
  await rm(debRoot, { recursive: true, force: true })
  await mkdir(path.join(debRoot, 'DEBIAN'), { recursive: true })
  await installLinuxPayload(debRoot, 'usr/bin')
  await cp(path.join(root, 'deploy/apt/control'), path.join(debRoot, 'DEBIAN/control'))
  for (const script of ['postinst', 'postrm', 'prerm']) {
    const target = path.join(debRoot, 'DEBIAN', script)
    await cp(path.join(root, 'deploy/apt', script), target)
    await chmod(target, 0o755)
  }
  await run('dpkg-deb', [
    '--build',
    debRoot,
    path.join(installersDir, `mediagen_${version}_amd64.deb`),
  ])
}

async function createRpmIfPossible() {
  if (!commandAvailable('rpmbuild')) throw new Error('缺少 rpmbuild，无法生成 yum/rpm 安装包')
  const top = path.join(installersDir, 'rpmbuild')
  const sourcePayload = path.join(top, 'SOURCES/payload')
  const specs = path.join(top, 'SPECS')
  await rm(top, { recursive: true, force: true })
  await mkdir(sourcePayload, { recursive: true })
  await mkdir(specs, { recursive: true })
  await installLinuxPayload(sourcePayload, 'usr/bin')
  const specPath = path.join(specs, 'mediagen.spec')
  await writeFile(specPath, rpmSpec(), 'utf8')
  await run('rpmbuild', ['--define', `_topdir ${top}`, '-bb', specPath])
  await copyBuiltRpms(path.join(top, 'RPMS'))
}

async function installLinuxPayload(targetRoot, binDir) {
  await mkdir(path.join(targetRoot, binDir), { recursive: true })
  await mkdir(path.join(targetRoot, 'usr/share/mediagen'), { recursive: true })
  await mkdir(path.join(targetRoot, 'opt/mediagen'), { recursive: true })
  await mkdir(path.join(targetRoot, 'lib/systemd/system'), { recursive: true })
  await mkdir(path.join(targetRoot, 'etc/mediagen'), { recursive: true })
  await cp(
    path.join(releaseDir, 'bin/mediagen-server'),
    path.join(targetRoot, binDir, 'mediagen-server')
  )
  await cp(path.join(releaseDir, 'console'), path.join(targetRoot, 'usr/share/mediagen/console'), {
    recursive: true,
  })
  await cp(path.join(releaseDir, 'plugins'), path.join(targetRoot, 'opt/mediagen/plugins'), {
    recursive: true,
  })
  await copyDirIfExists(
    path.join(releaseDir, 'browser-runtime'),
    path.join(targetRoot, 'opt/mediagen/browser-runtime')
  )
  await cp(
    path.join(root, 'deploy/systemd/mediagen-server.service'),
    path.join(targetRoot, 'lib/systemd/system/mediagen-server.service')
  )
  await writeFile(path.join(targetRoot, 'etc/mediagen/mediagen.env'), linuxEnv(), 'utf8')
}

async function copyBuiltRpms(rpmsDir) {
  if (!existsSync(rpmsDir)) return
  const find = spawnSync('find', [rpmsDir, '-name', '*.rpm', '-type', 'f'], { encoding: 'utf8' })
  for (const file of find.stdout.split(/\r?\n/).filter(Boolean)) {
    await cp(file, path.join(installersDir, path.basename(file)))
  }
}

async function copyIfExists(from, to) {
  if (existsSync(from)) await copyFile(from, to)
}

async function copyDirIfExists(from, to) {
  if (existsSync(from)) await cp(from, to, { recursive: true })
}

function macInfoPlist() {
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>zh_CN</string>
  <key>CFBundleDisplayName</key><string>MediaGen</string>
  <key>CFBundleExecutable</key><string>MediaGen</string>
  <key>CFBundleIconFile</key><string>mediagen</string>
  <key>CFBundleIdentifier</key><string>com.jjh886.mediagen.webconsole</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSUIElement</key><true/>
  <key>CFBundleName</key><string>MediaGen</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
</dict>
</plist>
`
}

function linuxEnv() {
  return [
    'MEDIAGEN_HOST=0.0.0.0',
    'MEDIAGEN_PORT=18080',
    'MEDIAGEN_CONSOLE_DIR=/usr/share/mediagen/console',
    'MEDIAGEN_PLUGINS_DIR=/opt/mediagen/plugins',
    'MEDIAGEN_DATA_DIR=/var/lib/mediagen',
    'MEDIAGEN_LOGS_DIR=/var/log/mediagen',
    'MEDIAGEN_PROFILES_DIR=/var/lib/mediagen/profiles',
    'CLOAK_BROWSER_PATH=/opt/mediagen/browser-runtime/chrome',
    '',
  ].join('\n')
}

function rpmSpec() {
  return `Name: mediagen
Version: ${version}
Release: 1%{?dist}
Summary: MediaGen Web Console server
License: Open Source

%description
MediaGen Web Console server with plugin execution, browser sessions, and CloakBrowser runtime.

%install
mkdir -p %{buildroot}
cp -a %{_sourcedir}/payload/* %{buildroot}/

%post
id mediagen >/dev/null 2>&1 || useradd --system --home /var/lib/mediagen --shell /sbin/nologin mediagen
mkdir -p /var/lib/mediagen/profiles /var/log/mediagen
chown -R mediagen:mediagen /var/lib/mediagen /var/log/mediagen
systemctl daemon-reload || true
systemctl enable mediagen-server.service || true

%preun
systemctl stop mediagen-server.service || true

%postun
systemctl daemon-reload || true

%files
/usr/bin/mediagen-server
/usr/share/mediagen/console
/opt/mediagen/plugins
/opt/mediagen/browser-runtime
/lib/systemd/system/mediagen-server.service
/etc/mediagen/mediagen.env
`
}

function windowsInstallCmd() {
  return [
    '@echo off',
    'powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-windows.ps1"',
    'exit /b %ERRORLEVEL%',
    '',
  ].join('\r\n')
}

function windowsInstallPs1(payloadZip) {
  const zipName = path.basename(payloadZip)
  return [
    '$ErrorActionPreference = "Stop"',
    '$Target = Join-Path $env:LOCALAPPDATA "MediaGen"',
    '$Payload = Join-Path $PSScriptRoot "' + zipName + '"',
    'if (Test-Path $Target) { Remove-Item -Recurse -Force $Target }',
    'New-Item -ItemType Directory -Force -Path $Target | Out-Null',
    'Expand-Archive -Force -Path $Payload -DestinationPath $Target',
    '$AppRoot = Join-Path $Target "MediaGen-local"',
    'New-MediaGenShortcut -Folder ([Environment]::GetFolderPath("Desktop"))',
    'New-MediaGenShortcut -Folder (Join-Path $env:APPDATA "Microsoft\\Windows\\Start Menu\\Programs")',
    'Start-Process (Join-Path $AppRoot "MediaGen.exe")',
    '',
    'function New-MediaGenShortcut {',
    '  param([string]$Folder)',
    '  New-Item -ItemType Directory -Force -Path $Folder | Out-Null',
    '  $Shell = New-Object -ComObject WScript.Shell',
    '  $Shortcut = $Shell.CreateShortcut((Join-Path $Folder "MediaGen.lnk"))',
    '  $Shortcut.TargetPath = Join-Path $AppRoot "MediaGen.exe"',
    '  $Shortcut.WorkingDirectory = $AppRoot',
    '  $Shortcut.IconLocation = Join-Path $AppRoot "icons\\mediagen.ico"',
    '  $Shortcut.Save()',
    '}',
    '',
  ].join('\r\n')
}

function iexpressSed(staging, payloadZip, exePath) {
  const cmd = path.join(staging, 'install-windows.cmd')
  const ps1 = path.join(staging, 'install-windows.ps1')
  return `[Version]
Class=IEXPRESS
SEDVersion=3
[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=0
HideExtractAnimation=1
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=
DisplayLicense=
FinishMessage=MediaGen 已安装完成。
TargetName=${exePath}
FriendlyName=MediaGen
AppLaunched=install-windows.cmd
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
SourceFiles=SourceFiles
[SourceFiles]
SourceFiles0=${staging}\\
[SourceFiles0]
${path.basename(payloadZip)}=${payloadZip}
install-windows.cmd=${cmd}
install-windows.ps1=${ps1}
`
}

function commandAvailable(command) {
  const checker = process.platform === 'win32' ? 'where' : 'which'
  const result = spawnSync(checker, [command], {
    shell: process.platform === 'win32',
    stdio: 'ignore',
  })
  return result.status === 0
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

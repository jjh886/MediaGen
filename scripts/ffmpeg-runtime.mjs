import { createHash } from 'node:crypto'
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { chmod, copyFile, mkdir, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import path from 'node:path'

const require = createRequire(import.meta.url)
const encoders = ['libwebp', 'libopus', 'aac', 'flac', 'pcm_s16le']
const executableName = process.platform === 'win32' ? 'ffmpeg.exe' : 'ffmpeg'
const probeExecutableName = process.platform === 'win32' ? 'ffprobe.exe' : 'ffprobe'
const metadataName = 'mediagen-ffmpeg.json'

export function validateFfmpegEncoders(output) {
  const available = new Set(
    [...output.matchAll(/^\s*[A-Z.]{6}\s+(\S+)/gm)].map((match) => match[1])
  )
  const missing = encoders.filter((encoder) => !available.has(encoder))
  if (missing.length) throw new Error(`FFmpeg 缺少客户端需要的编码器：${missing.join(', ')}`)
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    timeout: 30_000,
    maxBuffer: 8 * 1024 * 1024,
    ...options,
  })
  if (result.error || result.status !== 0) {
    throw new Error(
      `${path.basename(command)} 校验失败：${result.error?.message || result.stderr?.toString().trim() || result.status}`
    )
  }
  return result.stdout
}

function assertPortable(binary) {
  if (process.platform === 'darwin') {
    const libraries = run('otool', ['-L', binary])
      .toString()
      .split('\n')
      .slice(1)
      .map((line) => line.trim())
      .filter(Boolean)
    if (libraries.some((line) => !/^\/(usr\/lib|System\/Library)\//.test(line))) {
      throw new Error('FFmpeg 引用了 macOS 系统以外的动态库，不能作为独立运行时打包')
    }
  } else if (process.platform === 'linux') {
    const result = spawnSync('ldd', [binary], {
      encoding: 'utf8',
      timeout: 10_000,
      env: { ...process.env, LC_ALL: 'C' },
    })
    if (
      result.error ||
      !/not a dynamic executable|statically linked/.test(result.stdout + result.stderr)
    ) {
      throw new Error('Linux FFmpeg 必须使用静态构建，不能依赖用户安装额外动态库')
    }
  }
}

export function probeFfmpeg(
  binary,
  probeBinary = path.join(path.dirname(binary), probeExecutableName)
) {
  // Windows 保留 SystemRoot 等系统变量，清除构建机自定义的动态库与程序搜索路径。
  const searchPaths = ['PATH', 'DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH', 'LD_LIBRARY_PATH']
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !searchPaths.includes(key.toUpperCase()))
  )
  env.PATH = ''
  assertPortable(binary)
  assertPortable(probeBinary)
  const version = run(binary, ['-version'], { env }).toString().split('\n')[0]
  const probeVersion = run(probeBinary, ['-version'], { env }).toString().split('\n')[0]
  validateFfmpegEncoders(run(binary, ['-hide_banner', '-encoders'], { env }).toString())
  const image = run(
    binary,
    [
      '-v',
      'error',
      '-nostdin',
      '-f',
      'lavfi',
      '-i',
      'color=c=red:s=32x32',
      '-frames:v',
      '1',
      '-vf',
      'scale=16:16',
      '-c:v',
      'libwebp',
      '-q:v',
      '50',
      '-f',
      'webp',
      'pipe:1',
    ],
    { env }
  )
  if (image.toString('ascii', 0, 4) !== 'RIFF' || image.toString('ascii', 8, 12) !== 'WEBP') {
    throw new Error('包内 FFmpeg 的 WebP 缩放压缩验证失败')
  }
  const inspected = JSON.parse(
    run(
      probeBinary,
      [
        '-v',
        'error',
        '-show_entries',
        'stream=codec_name,width,height',
        '-of',
        'json',
        '-i',
        'pipe:0',
      ],
      { env, input: image }
    ).toString()
  )
  const stream = inspected.streams?.[0]
  if (stream?.codec_name !== 'webp' || stream.width !== 16 || stream.height !== 16) {
    throw new Error('包内 FFprobe 的媒体信息检查失败')
  }
  return { version, ffprobe_version: probeVersion, encoders }
}

async function bundleTool(outputDir, packageName, name) {
  const packageFile = require.resolve(`${packageName}/package.json`)
  const packageDir = path.dirname(packageFile)
  const pkg = JSON.parse(readFileSync(packageFile, 'utf8'))
  const fileName = process.platform === 'win32' ? `${name}.exe` : name
  const binary = path.join(packageDir, fileName)
  if (!existsSync(binary)) {
    // 支持构建机用 npm ci --ignore-scripts 安装依赖后再打包。
    const env = { ...process.env }
    for (const key of [
      `${name.toUpperCase()}_BIN`,
      `${name.toUpperCase()}_BINARY_RELEASE`,
      `${name.toUpperCase()}_BINARIES_URL`,
      'npm_config_platform',
      'npm_config_arch',
    ])
      delete env[key]
    await new Promise((resolve, reject) => {
      const child = spawn(process.execPath, [path.join(packageDir, 'install.js')], {
        env,
        stdio: 'inherit',
      })
      child.on('error', reject)
      child.on('exit', (code) =>
        code === 0 ? resolve() : reject(new Error(`下载静态 ${name} 失败：${code}`))
      )
    })
  }
  await mkdir(outputDir, { recursive: true })
  const bundled = path.join(outputDir, fileName)
  await copyFile(binary, bundled)
  await chmod(bundled, 0o755)
  for (const suffix of ['LICENSE', 'README']) {
    await copyFile(
      path.join(packageDir, `${fileName}.${suffix}`),
      path.join(outputDir, `${name}.${suffix}`)
    )
  }
  await copyFile(path.join(packageDir, 'LICENSE'), path.join(outputDir, 'LICENSE'))
  if (process.platform === 'darwin') run('codesign', ['--force', '--sign', '-', bundled])
  return { binary: bundled, pkg }
}

export async function bundleFfmpegRuntime(outputDir) {
  const { binary: bundled, pkg } = await bundleTool(outputDir, 'ffmpeg-static', 'ffmpeg')
  const { binary: probe, pkg: probePkg } = await bundleTool(
    outputDir,
    '@derhuerst/ffprobe-static',
    'ffprobe'
  )
  const capabilities = probeFfmpeg(bundled, probe)
  await writeFile(
    path.join(outputDir, metadataName),
    JSON.stringify(
      {
        ...capabilities,
        package: pkg.name,
        package_version: pkg.version,
        ffprobe_package: probePkg.name,
        ffprobe_package_version: probePkg.version,
        release: pkg['ffmpeg-static']['binary-release-tag'],
        source: 'https://github.com/eugeneware/ffmpeg-static',
        platform: process.platform,
        arch: process.arch,
        sha256: createHash('sha256').update(readFileSync(bundled)).digest('hex'),
        ffprobe_sha256: createHash('sha256').update(readFileSync(probe)).digest('hex'),
      },
      null,
      2
    ) + '\n'
  )
  console.log(
    `已打包独立 FFmpeg 运行时：${capabilities.version} / ${process.platform}-${process.arch}`
  )
}

export function verifyFfmpegRuntime(runtimeDir) {
  for (const name of [
    executableName,
    probeExecutableName,
    metadataName,
    'ffmpeg.LICENSE',
    'ffmpeg.README',
    'ffprobe.LICENSE',
    'ffprobe.README',
    'LICENSE',
  ]) {
    if (!existsSync(path.join(runtimeDir, name)))
      throw new Error(`FFmpeg 运行时缺少 ${name}：${runtimeDir}`)
  }
  const binary = path.join(runtimeDir, executableName)
  const metadata = JSON.parse(readFileSync(path.join(runtimeDir, metadataName), 'utf8'))
  const sha256 = createHash('sha256').update(readFileSync(binary)).digest('hex')
  const probe = path.join(runtimeDir, probeExecutableName)
  const probeSha256 = createHash('sha256').update(readFileSync(probe)).digest('hex')
  if (
    metadata.platform !== process.platform ||
    metadata.arch !== process.arch ||
    metadata.sha256 !== sha256 ||
    metadata.ffprobe_sha256 !== probeSha256
  ) {
    throw new Error('FFmpeg 运行时平台或文件校验不匹配')
  }
  return probeFfmpeg(binary, probe)
}

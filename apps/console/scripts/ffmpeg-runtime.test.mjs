import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import {
  bundleFfmpegRuntime,
  validateFfmpegEncoders,
  verifyFfmpegRuntime,
} from '../../../scripts/ffmpeg-runtime.mjs'

describe('安装包内独立 FFmpeg 运行时', () => {
  let root
  let runtime
  beforeAll(async () => {
    root = await mkdtemp(path.join(os.tmpdir(), 'mediagen media runtime '))
    runtime = path.join(root, '安装后的客户端', 'ffmpeg-runtime')
    await bundleFfmpegRuntime(runtime)
  }, 60_000)
  afterAll(async () => {
    if (root) await rm(root, { recursive: true, force: true })
  })

  it('复制到含空格的安装路径后，不借助 PATH 即可缩放、压缩 WebP', () => {
    const result = verifyFfmpegRuntime(runtime)
    expect(result.version).toMatch(/^ffmpeg version /)
    expect(result.ffprobe_version).toMatch(/^ffprobe version /)
    expect(result.encoders).toContain('libwebp')
    expect(result.encoders).toContain('libopus')
  })

  it('拒绝缺失文件或文件校验不一致的运行时', async () => {
    expect(() => verifyFfmpegRuntime(root)).toThrow('FFmpeg 运行时缺少')
    const probe = path.join(runtime, process.platform === 'win32' ? 'ffprobe.exe' : 'ffprobe')
    const moved = path.join(root, 'probe-backup')
    await rename(probe, moved)
    try {
      expect(() => verifyFfmpegRuntime(runtime)).toThrow('FFmpeg 运行时缺少 ffprobe')
    } finally {
      await rename(moved, probe)
    }
    const metadataPath = path.join(runtime, 'mediagen-ffmpeg.json')
    const saved = await readFile(metadataPath, 'utf8')
    try {
      for (const field of ['sha256', 'ffprobe_sha256', 'arch']) {
        await writeFile(metadataPath, JSON.stringify({ ...JSON.parse(saved), [field]: 'invalid' }))
        expect(() => verifyFfmpegRuntime(runtime)).toThrow('文件校验不匹配')
      }
    } finally {
      await writeFile(metadataPath, saved)
    }
  })

  it('仅有同名命令不够，缺少 libwebp 或音频编码器时阻止打包', () => {
    expect(() => validateFfmpegEncoders(' V....D libx264 encoder')).toThrow('libwebp')
    expect(() => validateFfmpegEncoders(' V....D libwebp encoder')).toThrow('libopus')
    expect(() =>
      validateFfmpegEncoders(
        [
          ' V....D libwebp encoder',
          ' A....D libopus encoder',
          ' A..... aac encoder',
          ' A....D flac encoder',
          ' A....D pcm_s16le encoder',
        ].join('\n')
      )
    ).not.toThrow()
  })
})

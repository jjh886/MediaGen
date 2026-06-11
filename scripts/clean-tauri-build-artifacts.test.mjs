import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'

import { cleanupTauriBuildArtifacts } from './clean-tauri-build-artifacts.mjs'

let tmpDir

afterEach(async () => {
  if (tmpDir) {
    await fs.rm(tmpDir, { recursive: true, force: true })
    tmpDir = undefined
  }
})

async function touch(filePath) {
  await fs.mkdir(path.dirname(filePath), { recursive: true })
  await fs.writeFile(filePath, 'x')
}

async function exists(filePath) {
  try {
    await fs.access(filePath)
    return true
  } catch {
    return false
  }
}

describe('cleanupTauriBuildArtifacts', () => {
  it('removes non-delivery tauri build artifacts and keeps delivery bundles', async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'mediagen-clean-tauri-'))
    const desktopDir = path.join(tmpDir, 'desktop')
    const targetDir = path.join(desktopDir, 'src-tauri', 'target')

    const removeFiles = [
      path.join(desktopDir, 'dist', 'index.html'),
      path.join(targetDir, 'debug', 'mediagen'),
      path.join(targetDir, 'llvm-cov-target', 'src-tauri.profdata'),
      path.join(targetDir, 'release', 'build', 'dep', 'out'),
      path.join(targetDir, 'release', 'deps', 'libmediagen.rlib'),
      path.join(targetDir, 'release', 'incremental', 'cache.bin'),
      path.join(targetDir, 'release', '.fingerprint', 'mediagen', 'fingerprint'),
      path.join(targetDir, 'release', 'examples', 'debug_login'),
      path.join(targetDir, 'release', 'native', 'libsqlite.a'),
      path.join(targetDir, 'release', 'plugins', 'GPTImage2', 'manifest.json'),
    ]
    const keepFiles = [
      path.join(targetDir, 'release', 'mediagen'),
      path.join(targetDir, 'release', 'mediagen.exe'),
      path.join(targetDir, 'release', 'bundle', 'macos', 'MediaGen.app', 'Contents', 'Info.plist'),
      path.join(targetDir, 'release', 'bundle', 'dmg', 'MediaGen.dmg'),
      path.join(targetDir, 'release', 'bundle', 'nsis', 'MediaGen_0.1.0_x64-setup.exe'),
      path.join(targetDir, 'release', 'bundle', 'msi', 'MediaGen_0.1.0_x64_en-US.msi'),
    ]

    for (const filePath of [...removeFiles, ...keepFiles]) {
      await touch(filePath)
    }

    const result = await cleanupTauriBuildArtifacts({ desktopDir })

    for (const filePath of removeFiles) {
      expect(await exists(filePath)).toBe(false)
    }
    for (const filePath of keepFiles) {
      expect(await exists(filePath)).toBe(true)
    }
    expect(result.removed).toEqual(
      expect.arrayContaining([
        path.join(desktopDir, 'dist'),
        path.join(targetDir, 'debug'),
        path.join(targetDir, 'llvm-cov-target'),
        path.join(targetDir, 'release', 'plugins'),
      ])
    )
  })

  it('ignores missing artifact directories', async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'mediagen-clean-tauri-empty-'))
    const desktopDir = path.join(tmpDir, 'desktop')
    await fs.mkdir(desktopDir, { recursive: true })

    await expect(cleanupTauriBuildArtifacts({ desktopDir })).resolves.toEqual({
      removed: [],
      skipped: expect.any(Array),
    })
  })
})

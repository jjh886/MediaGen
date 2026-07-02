import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  checkBrowserRuntimeAvailable: vi.fn(),
  getBrowserRuntimeInstallGuide: vi.fn(),
  installBrowserRuntime: vi.fn(),
}))

vi.mock('@/shared/services/settings-service', () => ({
  checkBrowserRuntimeAvailable: mocks.checkBrowserRuntimeAvailable,
  getBrowserRuntimeInstallGuide: mocks.getBrowserRuntimeInstallGuide,
  installBrowserRuntime: mocks.installBrowserRuntime,
}))

describe('startup-service', () => {
  beforeEach(() => {
    vi.resetModules()
    mocks.checkBrowserRuntimeAvailable.mockReset()
    mocks.getBrowserRuntimeInstallGuide.mockReset()
    mocks.installBrowserRuntime.mockReset()
  })

  it('CloakBrowser 健康时允许进入应用', async () => {
    mocks.checkBrowserRuntimeAvailable.mockResolvedValue({
      ok: true,
      message: 'CloakBrowser 130.0',
    })

    const mod = await import('../startup-service')
    await expect(mod.checkStartupReadiness()).resolves.toEqual({ ok: true })
  })

  it('CloakBrowser 不可用时返回安装阻断信息', async () => {
    mocks.checkBrowserRuntimeAvailable.mockResolvedValue({
      ok: false,
      message: '未检测到 CloakBrowser',
    })

    const mod = await import('../startup-service')
    await expect(mod.checkStartupReadiness()).resolves.toEqual({
      ok: false,
      reason: 'browser-runtime',
      title: '未检测到 CloakBrowser',
      message: '未检测到 CloakBrowser',
    })
  })

  it('安装引导和自动安装方法直接复用设置服务', async () => {
    const guide = { runtime: 'CloakBrowser', status: 'missing', steps: [], notes: [] }
    const runtime = { id: 'cloakbrowser', healthy: true }
    mocks.getBrowserRuntimeInstallGuide.mockResolvedValue(guide)
    mocks.installBrowserRuntime.mockResolvedValue(runtime)

    const mod = await import('../startup-service')

    await expect(mod.getBrowserRuntimeInstallGuide()).resolves.toBe(guide)
    await expect(mod.installBrowserRuntime()).resolves.toBe(runtime)
  })
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mockInvoke = vi.fn()

function enableTauriRuntime() {
  const tauriWindow = window as Record<string, unknown>
  tauriWindow.__TAURI_INTERNALS__ = {}
}

vi.mock('@/shared/services/tauri-bridge', () => ({
  tauriInvoke: (...args: unknown[]) => mockInvoke(...args),
}))

beforeEach(() => {
  vi.resetModules()
  mockInvoke.mockReset()
  localStorage.clear()
  delete (window as Record<string, unknown>).__TAURI_INTERNALS__
})

afterEach(() => {
  delete (window as Record<string, unknown>).__TAURI_INTERNALS__
})

describe('checkStartupReadiness', () => {
  it('非 Tauri 环境直接放行', async () => {
    const mod = await import('../startup-service')
    const result = await mod.checkStartupReadiness()

    expect(result.ok).toBe(true)
    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it('Chrome 不可用时拦截启动', async () => {
    enableTauriRuntime()
    mockInvoke.mockRejectedValueOnce('未检测到 Chrome')

    const mod = await import('../startup-service')
    const result = await mod.checkStartupReadiness()

    expect(result).toMatchObject({
      ok: false,
      reason: 'chrome',
      title: '未检测到 Chrome 浏览器',
      message: '未检测到 Chrome',
    })
    expect(mockInvoke).toHaveBeenCalledWith('check_chrome_available')
    expect(mockInvoke).not.toHaveBeenCalledWith('check_macos_startup_permissions', {
      prompt: false,
    })
  })

  it('Tauri 桌面环境会先检查 Chrome，再检查 macOS 权限', async () => {
    enableTauriRuntime()
    mockInvoke
      .mockResolvedValueOnce('Google Chrome 130')
      .mockResolvedValueOnce({ required: false, granted: true, permissions: [] })

    const mod = await import('../startup-service')
    await mod.checkStartupReadiness()

    expect(mockInvoke).toHaveBeenNthCalledWith(1, 'check_chrome_available')
    expect(mockInvoke).toHaveBeenNthCalledWith(2, 'check_macos_startup_permissions', {
      prompt: false,
    })
  })

  it('macOS 必需权限缺失时拦截启动', async () => {
    enableTauriRuntime()
    mockInvoke.mockResolvedValueOnce('Google Chrome 130').mockResolvedValueOnce({
      required: true,
      granted: false,
      permissions: [
        {
          key: 'app_management',
          title: 'App 管理权限',
          granted: false,
          blocking: true,
          message: '需要授权',
        },
      ],
    })

    const mod = await import('../startup-service')
    const result = await mod.checkStartupReadiness()

    expect(result.ok).toBe(false)
    expect(result.reason).toBe('macos-permission')
    expect(result.permissions?.[0].key).toBe('app_management')
    expect(mockInvoke).toHaveBeenNthCalledWith(2, 'check_macos_startup_permissions', {
      prompt: false,
    })
  })

  it('用户确认已开启 App 管理并重启后允许进入主应用', async () => {
    enableTauriRuntime()
    mockInvoke.mockResolvedValueOnce('Google Chrome 130').mockResolvedValueOnce({
      required: true,
      granted: false,
      permissions: [
        {
          key: 'app_management',
          title: 'App 管理权限',
          granted: false,
          blocking: true,
          message: '需要授权',
        },
      ],
    })

    const mod = await import('../startup-service')
    mod.confirmMacosAppManagementPermission()
    const result = await mod.checkStartupReadiness()

    expect(result.ok).toBe(true)
  })

  it('Chrome 和 macOS 权限都通过时放行', async () => {
    enableTauriRuntime()
    mockInvoke
      .mockResolvedValueOnce('Google Chrome 130')
      .mockResolvedValueOnce({ required: true, granted: true, permissions: [] })

    const mod = await import('../startup-service')
    const result = await mod.checkStartupReadiness()

    expect(result.ok).toBe(true)
  })
})

describe('macOS permission helpers', () => {
  it('请求权限时会带 prompt=true', async () => {
    enableTauriRuntime()
    mockInvoke.mockResolvedValueOnce({ required: true, granted: false, permissions: [] })

    const mod = await import('../startup-service')
    await mod.requestMacosStartupPermissions()

    expect(mockInvoke).toHaveBeenCalledWith('check_macos_startup_permissions', { prompt: true })
  })

  it('打开系统设置时调用 Tauri 命令', async () => {
    enableTauriRuntime()
    mockInvoke.mockResolvedValueOnce(undefined)

    const mod = await import('../startup-service')
    await mod.openMacosPrivacySettings()

    expect(mockInvoke).toHaveBeenCalledWith('open_macos_privacy_settings')
  })
})

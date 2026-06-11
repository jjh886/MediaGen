import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import {
  getSetting,
  setSetting,
  getAllSettings,
  checkChromeVersion,
  getCacheSize,
  clearCache,
} from '../settings-service'

const mockInvoke = vi.fn()

vi.mock('@/shared/services/tauri-bridge', () => ({
  tauriInvoke: (...args: unknown[]) => mockInvoke(...args),
}))

beforeEach(() => {
  mockInvoke.mockReset()
  localStorage.clear()
})

describe('getSetting', () => {
  it('非 Tauri 环境返回 localStorage 中的值', async () => {
    localStorage.setItem('settings:api_port', '8080')
    const result = await getSetting('api_port')
    expect(result).toBe(8080)
  })

  it('非 Tauri 环境 localStorage 无值时返回默认值', async () => {
    const result = await getSetting('api_port')
    expect(result).toBe(18080)
  })

  it('非 Tauri 环境 localStorage 值解析失败时返回默认值', async () => {
    localStorage.setItem('settings:api_port', '{invalid json')
    const result = await getSetting('api_port')
    expect(result).toBe(18080)
  })

  it('非 Tauri 环境返回字符串类型默认值', async () => {
    const result = await getSetting('cache_dir')
    expect(result).toBe('')
  })

  it('非 Tauri 环境返回字符串原值，不把 JSON 字符串解析成对象', async () => {
    const overrides = '{"chatgpt_free":{"concurrency_limit":0}}'
    localStorage.setItem('settings:platform_tier_overrides', overrides)
    const result = await getSetting('platform_tier_overrides')
    expect(result).toBe(overrides)
  })
})

describe('setSetting', () => {
  it('非 Tauri 环境将值写入 localStorage', async () => {
    await setSetting('api_port', 8080)
    expect(localStorage.getItem('settings:api_port')).toBe('8080')
  })

  it('非 Tauri 环境写入字符串类型值', async () => {
    await setSetting('cache_dir', '/tmp/cache')
    expect(localStorage.getItem('settings:cache_dir')).toBe('/tmp/cache')
  })
})

describe('getAllSettings', () => {
  it('返回所有设置项（使用默认值）', async () => {
    const settings = await getAllSettings()
    expect(settings.api_port).toBe(18080)
    expect(settings.api_end_port).toBe(18089)
    expect(settings.api_ports).toBe('')
    expect(settings.api_port_count).toBe(10)
    expect(settings.api_address).toBe('127.0.0.1')
    expect(settings.api_allowed_origins).toBe('*')
    expect(settings.image_task_concurrency).toBe(64)
    expect(settings.image_task_queue_limit).toBe(512)
    expect(settings.check_interval_minutes).toBe(30)
    expect(settings.cache_dir).toBe('')
  })

  it('localStorage 中有部分值时混合返回', async () => {
    localStorage.setItem('settings:api_port', '3000')
    localStorage.setItem('settings:check_interval_minutes', '5')
    const settings = await getAllSettings()
    expect(settings.api_port).toBe(3000)
    expect(settings.check_interval_minutes).toBe(5)
    expect(settings.api_address).toBe('127.0.0.1')
    expect(settings.api_end_port).toBe(18089)
    expect(settings.api_port_count).toBe(10)
  })
})

describe('checkChromeVersion', () => {
  it('非 Tauri 环境返回 mock 版本', async () => {
    const result = await checkChromeVersion('/usr/bin/chrome')
    expect(result).toBe('Chrome 124.0.6367.91 (mock)')
  })
})

describe('getCacheSize', () => {
  it('非 Tauri 环境返回 mock 大小', async () => {
    const result = await getCacheSize()
    expect(result).toBe('256 MB (mock)')
  })
})

describe('clearCache', () => {
  it('非 Tauri 环境直接返回', async () => {
    await expect(clearCache()).resolves.toBeUndefined()
  })
})

// ─── Tauri 环境测试（通过 resetModules + 动态 import 重新加载模块）───

describe('getSetting (Tauri 环境)', () => {
  beforeEach(() => {
    vi.resetModules()
    const win = window as Record<string, unknown>
    win.__TAURI_INTERNALS__ = {}
  })

  afterEach(() => {
    delete (window as Record<string, unknown>).__TAURI_INTERNALS__
  })

  it('invoke 返回数字字符串时 JSON.parse 为数字', async () => {
    const localMock = vi.fn().mockResolvedValue('8080')
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    const result = await mod.getSetting('api_port')
    expect(localMock).toHaveBeenCalledWith('get_settings', { key: 'api_port' })
    expect(result).toBe(8080)
  })

  it('invoke 返回普通字符串时直接返回', async () => {
    const localMock = vi.fn().mockResolvedValue('/custom/path')
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    const result = await mod.getSetting('cache_dir')
    expect(result).toBe('/custom/path')
  })

  it('invoke 返回 JSON 对象字符串时，字符串配置仍保持原文', async () => {
    const overrides = '{"chatgpt_free":{"concurrency_limit":0}}'
    const localMock = vi.fn().mockResolvedValue(overrides)
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    const result = await mod.getSetting('platform_tier_overrides')
    expect(result).toBe(overrides)
  })

  it('invoke 返回 null 时返回默认值', async () => {
    const localMock = vi.fn().mockResolvedValue(null)
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    const result = await mod.getSetting('api_port')
    expect(result).toBe(18080)
  })

  it('invoke 抛错时返回默认值', async () => {
    const localMock = vi.fn().mockRejectedValue(new Error('fail'))
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    const result = await mod.getSetting('api_port')
    expect(result).toBe(18080)
  })
})

describe('setSetting (Tauri 环境)', () => {
  beforeEach(() => {
    vi.resetModules()
    const win = window as Record<string, unknown>
    win.__TAURI_INTERNALS__ = {}
  })

  afterEach(() => {
    delete (window as Record<string, unknown>).__TAURI_INTERNALS__
  })

  it('invoke 成功时调用 set_setting', async () => {
    const localMock = vi.fn().mockResolvedValue(undefined)
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    await mod.setSetting('api_port', 3000)
    expect(localMock).toHaveBeenCalledWith('set_setting', { key: 'api_port', value: '3000' })
  })

  it('invoke 抛错时 fallback 到 localStorage', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const localMock = vi.fn().mockRejectedValue(new Error('fail'))
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    await mod.setSetting('api_port', 3000)
    expect(localStorage.getItem('settings:api_port')).toBe('3000')
    warnSpy.mockRestore()
  })
})

describe('checkChromeVersion (Tauri 环境)', () => {
  beforeEach(() => {
    vi.resetModules()
    const win = window as Record<string, unknown>
    win.__TAURI_INTERNALS__ = {}
  })

  afterEach(() => {
    delete (window as Record<string, unknown>).__TAURI_INTERNALS__
  })

  it('invoke 成功返回版本', async () => {
    const localMock = vi.fn().mockResolvedValue('Chrome 130.0')
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    const result = await mod.checkChromeVersion('/usr/bin/chrome')
    expect(result).toBe('Chrome 130.0')
  })

  it('invoke 抛错返回检测失败', async () => {
    const localMock = vi.fn().mockRejectedValue(new Error('fail'))
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    const result = await mod.checkChromeVersion('/usr/bin/chrome')
    expect(result).toBe('检测失败')
  })
})

describe('getCacheSize (Tauri 环境)', () => {
  beforeEach(() => {
    vi.resetModules()
    const win = window as Record<string, unknown>
    win.__TAURI_INTERNALS__ = {}
  })

  afterEach(() => {
    delete (window as Record<string, unknown>).__TAURI_INTERNALS__
  })

  it('invoke 成功返回大小', async () => {
    const localMock = vi.fn().mockResolvedValue('512 MB')
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    const result = await mod.getCacheSize()
    expect(result).toBe('512 MB')
  })

  it('invoke 抛错返回未知', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const localMock = vi.fn().mockRejectedValue(new Error('fail'))
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    const result = await mod.getCacheSize()
    expect(result).toBe('未知')
    warnSpy.mockRestore()
  })
})

describe('clearCache (Tauri 环境)', () => {
  beforeEach(() => {
    vi.resetModules()
    const win = window as Record<string, unknown>
    win.__TAURI_INTERNALS__ = {}
  })

  afterEach(() => {
    delete (window as Record<string, unknown>).__TAURI_INTERNALS__
  })

  it('invoke 调用 clear_cache', async () => {
    const localMock = vi.fn().mockResolvedValue(undefined)
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const mod = await import('../settings-service')
    await mod.clearCache()
    expect(localMock).toHaveBeenCalledWith('clear_cache')
  })
})

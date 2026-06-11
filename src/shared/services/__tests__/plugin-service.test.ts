import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { fetchPlugins } from '../plugin-service'

const mockInvoke = vi.fn()

vi.mock('../tauri-bridge', () => ({
  tauriInvoke: (...args: unknown[]) => mockInvoke(...args),
}))

beforeEach(() => {
  mockInvoke.mockReset()
})

describe('fetchPlugins', () => {
  it('非 Tauri 环境返回 mock 插件列表', async () => {
    const plugins = await fetchPlugins()
    expect(plugins).toHaveLength(5)
    expect(plugins[0]).toEqual({
      platform_id: 'chatgpt',
      display_name: 'ChatGPT',
      model_count: 1,
      supports_registration: false,
    })
    expect(plugins.map((p) => p.platform_id)).toEqual([
      'chatgpt',
      'codex',
      'jimeng',
      'doubao',
      'flow',
    ])
  })

  it('非 Tauri 环境 mock 数据包含正确的字段', async () => {
    const plugins = await fetchPlugins()
    for (const p of plugins) {
      expect(p).toHaveProperty('platform_id')
      expect(p).toHaveProperty('display_name')
      expect(p).toHaveProperty('model_count')
      expect(p).toHaveProperty('supports_registration')
    }
  })

  it('非 Tauri 环境所有兜底插件都不支持注册', async () => {
    const plugins = await fetchPlugins()
    expect(plugins.every((p) => p.supports_registration === false)).toBe(true)
  })
})

describe('fetchPlugins (Tauri 环境)', () => {
  beforeEach(() => {
    vi.resetModules()
    const win = window as Record<string, unknown>
    win.__TAURI_INTERNALS__ = {}
  })

  afterEach(() => {
    delete (window as Record<string, unknown>).__TAURI_INTERNALS__
  })

  it('Tauri 环境调用 get_plugins 并返回结果', async () => {
    const fakePlugins = [
      { platform_id: 'test', display_name: 'Test', model_count: 1, supports_registration: false },
    ]
    const localMock = vi.fn().mockResolvedValue(fakePlugins)
    vi.doMock('../tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const { fetchPlugins: fetch } = await import('../plugin-service')
    const result = await fetch()
    expect(localMock).toHaveBeenCalledWith('get_plugins')
    expect(result).toEqual(fakePlugins)
  })

  it('Tauri 环境 invoke 返回 null 时兜底为空数组', async () => {
    const localMock = vi.fn().mockResolvedValue(null)
    vi.doMock('../tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const { fetchPlugins: fetch } = await import('../plugin-service')
    const result = await fetch()
    expect(result).toEqual([])
  })

  it('Tauri 环境 invoke 抛错时返回空数组', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const localMock = vi.fn().mockRejectedValue(new Error('fail'))
    vi.doMock('../tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const { fetchPlugins: fetch } = await import('../plugin-service')
    const result = await fetch()
    expect(result).toEqual([])
    expect(warnSpy).toHaveBeenCalled()
    warnSpy.mockRestore()
  })
})

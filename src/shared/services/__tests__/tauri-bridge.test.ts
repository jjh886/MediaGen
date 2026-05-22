import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

describe('tauri-bridge', () => {
  beforeEach(() => {
    vi.resetModules()
  })

  afterEach(() => {
    // 清理 __TAURI_INTERNALS__
    if ('__TAURI_INTERNALS__' in window) {
      delete (window as Record<string, unknown>).__TAURI_INTERNALS__
    }
  })

  it('非 Tauri 环境下返回 undefined 并输出 warn', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { tauriInvoke } = await import('../tauri-bridge')

    const result = await tauriInvoke('some_command', { key: 'value' })

    expect(result).toBeUndefined()
    expect(warnSpy).toHaveBeenCalledWith('[tauri-bridge] Tauri 不可用，跳过命令: some_command')
    warnSpy.mockRestore()
  })

  it('Tauri 环境下调用 invoke 并返回结果', async () => {
    const win = window as Record<string, unknown>
    win.__TAURI_INTERNALS__ = {}

    const mockInvoke = vi.fn().mockResolvedValue({ data: 'test' })
    vi.doMock('@tauri-apps/api/core', () => ({
      invoke: mockInvoke,
    }))

    const { tauriInvoke } = await import('../tauri-bridge')
    const result = await tauriInvoke<{ data: string }>('get_data', { id: '1' })

    expect(mockInvoke).toHaveBeenCalledWith('get_data', { id: '1' })
    expect(result).toEqual({ data: 'test' })
  })

  it('Tauri 环境下无参数调用', async () => {
    const win = window as Record<string, unknown>
    win.__TAURI_INTERNALS__ = {}

    const mockInvoke = vi.fn().mockResolvedValue('ok')
    vi.doMock('@tauri-apps/api/core', () => ({
      invoke: mockInvoke,
    }))

    const { tauriInvoke } = await import('../tauri-bridge')
    const result = await tauriInvoke<string>('ping')

    expect(mockInvoke).toHaveBeenCalledWith('ping', undefined)
    expect(result).toBe('ok')
  })
})

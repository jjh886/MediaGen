import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { fetchTasks, cancelTask } from '../task-service'

const mockInvoke = vi.fn()

vi.mock('@/shared/services/tauri-bridge', () => ({
  tauriInvoke: (...args: unknown[]) => mockInvoke(...args),
}))

beforeEach(() => {
  mockInvoke.mockReset()
})

describe('fetchTasks', () => {
  it('非 Tauri 环境返回全部 mock 任务', async () => {
    const tasks = await fetchTasks()
    expect(tasks).toHaveLength(2)
  })

  it('非 Tauri 环境空字符串 platform 返回全部任务', async () => {
    const tasks = await fetchTasks('')
    expect(tasks).toHaveLength(2)
  })

  it('非 Tauri 环境过滤不存在的平台返回空数组', async () => {
    const tasks = await fetchTasks('nonexistent')
    expect(tasks).toHaveLength(0)
  })

  it('非 Tauri 环境 flow 平台只有一个任务', async () => {
    const tasks = await fetchTasks('flow')
    expect(tasks).toHaveLength(1)
    expect(tasks[0].id).toBe('task-002')
  })

  it('mock 任务包含 processing 与 pending 两种状态', async () => {
    const tasks = await fetchTasks()
    const statuses = tasks.map((t) => t.status)
    expect(statuses).toContain('processing')
    expect(statuses).toContain('pending')
  })
})

describe('cancelTask', () => {
  it('非 Tauri 环境直接返回', async () => {
    await expect(cancelTask('task-001')).resolves.toBeUndefined()
    expect(mockInvoke).not.toHaveBeenCalled()
  })
})

describe('fetchTasks (Tauri 环境)', () => {
  beforeEach(() => {
    vi.resetModules()
    const win = window as Record<string, unknown>
    win.__TAURI_INTERNALS__ = {}
  })

  afterEach(() => {
    delete (window as Record<string, unknown>).__TAURI_INTERNALS__
  })

  it('Tauri 环境调用 get_tasks 并返回结果', async () => {
    const fakeTasks = [{ id: 't1', platform: 'flow' }]
    const localMock = vi.fn().mockResolvedValue(fakeTasks)
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const { fetchTasks: fetch } = await import('../task-service')
    const result = await fetch('flow', 10, 0)
    expect(localMock).toHaveBeenCalledWith('get_tasks', { platform: 'flow', limit: 10, offset: 0 })
    expect(result).toEqual(fakeTasks)
  })

  it('Tauri 环境空 platform 传 null', async () => {
    const localMock = vi.fn().mockResolvedValue([])
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const { fetchTasks: fetch } = await import('../task-service')
    await fetch('')
    expect(localMock).toHaveBeenCalledWith('get_tasks', { platform: null, limit: 50, offset: 0 })
  })

  it('Tauri 环境 invoke 返回 null 时兜底为空数组', async () => {
    const localMock = vi.fn().mockResolvedValue(null)
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const { fetchTasks: fetch } = await import('../task-service')
    const result = await fetch()
    expect(result).toEqual([])
  })
})

describe('cancelTask (Tauri 环境)', () => {
  beforeEach(() => {
    vi.resetModules()
    const win = window as Record<string, unknown>
    win.__TAURI_INTERNALS__ = {}
  })

  afterEach(() => {
    delete (window as Record<string, unknown>).__TAURI_INTERNALS__
  })

  it('Tauri 环境调用 cancel_task', async () => {
    const localMock = vi.fn().mockResolvedValue(undefined)
    vi.doMock('@/shared/services/tauri-bridge', () => ({
      tauriInvoke: localMock,
    }))
    const { cancelTask: cancel } = await import('../task-service')
    await cancel('task-123')
    expect(localMock).toHaveBeenCalledWith('cancel_task', { taskId: 'task-123' })
  })
})

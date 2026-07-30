import { act } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useGenerationQueueStore } from '../useGenerationQueueStore'

const mockFetchGenerationQueue = vi.fn()
const mockRunGenerationQueueAction = vi.fn()

vi.mock('../../services/generation-queue-service', () => ({
  fetchGenerationQueue: (...args: unknown[]) => mockFetchGenerationQueue(...args),
  runGenerationQueueAction: (...args: unknown[]) => mockRunGenerationQueueAction(...args),
}))

const RESPONSE = {
  total: 1,
  items: [
    {
      taskId: 'task-1',
      platform: 'chatgpt',
      modelId: 'gpt-image-1',
      prompt: '雨夜街道',
      status: 'queued' as const,
      position: 1,
      queuedAt: '2026-07-18T10:00:00Z',
    },
  ],
}

beforeEach(() => {
  mockFetchGenerationQueue.mockReset()
  mockRunGenerationQueueAction.mockReset()
  useGenerationQueueStore.setState({
    items: [],
    total: 0,
    limit: 100,
    offset: 0,
    loading: false,
    error: '',
  })
})

describe('useGenerationQueueStore', () => {
  it('加载真实队列数量和列表', async () => {
    mockFetchGenerationQueue.mockResolvedValue(RESPONSE)

    await act(async () => useGenerationQueueStore.getState().load())

    expect(useGenerationQueueStore.getState().items).toEqual(RESPONSE.items)
    expect(useGenerationQueueStore.getState().total).toBe(1)
    expect(mockFetchGenerationQueue).toHaveBeenCalledWith(100, 0)
  })

  it('批量操作完成后按当前分页重新加载', async () => {
    mockRunGenerationQueueAction.mockResolvedValue(undefined)
    mockFetchGenerationQueue.mockResolvedValue({ total: 0, items: [] })
    useGenerationQueueStore.setState({ limit: 20, offset: 40 })

    await act(async () =>
      useGenerationQueueStore.getState().runAction('promote', ['task-1', 'task-2'])
    )

    expect(mockRunGenerationQueueAction).toHaveBeenCalledWith('promote', ['task-1', 'task-2'])
    expect(mockFetchGenerationQueue).toHaveBeenCalledWith(20, 40)
  })

  it('空批量操作不请求后端', async () => {
    await act(async () => useGenerationQueueStore.getState().runAction('cancel', []))
    expect(mockRunGenerationQueueAction).not.toHaveBeenCalled()
  })

  it('加载失败时保留可读错误', async () => {
    mockFetchGenerationQueue.mockRejectedValue(new Error('读取失败'))

    await expect(useGenerationQueueStore.getState().load()).rejects.toThrow('读取失败')

    expect(useGenerationQueueStore.getState().error).toBe('读取失败')
    expect(useGenerationQueueStore.getState().loading).toBe(false)
  })
})

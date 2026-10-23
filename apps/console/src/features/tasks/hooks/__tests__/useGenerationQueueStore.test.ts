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
      taskKind: 'image' as const,
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
    taskKind: 'image',
    revision: 0,
    items: [],
    total: 0,
    limit: 40,
    offset: 0,
    loading: false,
    loadingMore: false,
    hasMore: true,
    error: '',
  })
})

describe('useGenerationQueueStore', () => {
  it('加载真实队列数量和列表', async () => {
    mockFetchGenerationQueue.mockResolvedValue(RESPONSE)

    await act(async () => useGenerationQueueStore.getState().load())

    expect(useGenerationQueueStore.getState().items).toEqual(RESPONSE.items)
    expect(useGenerationQueueStore.getState().total).toBe(1)
    expect(mockFetchGenerationQueue).toHaveBeenCalledWith(40, 0, 'image')
  })

  it('批量操作完成后回到最新四十条', async () => {
    mockRunGenerationQueueAction.mockResolvedValue(undefined)
    mockFetchGenerationQueue.mockResolvedValue({ total: 0, items: [] })
    useGenerationQueueStore.setState({ limit: 20, offset: 40 })

    await act(async () =>
      useGenerationQueueStore.getState().runAction('promote', ['task-1', 'task-2'])
    )

    expect(mockRunGenerationQueueAction).toHaveBeenCalledWith('promote', ['task-1', 'task-2'])
    expect(mockFetchGenerationQueue).toHaveBeenCalledWith(40, 0, 'image')
  })

  it('接近底部时按当前数量追加下一页', async () => {
    useGenerationQueueStore.setState({ items: RESPONSE.items, total: 2, hasMore: true, offset: 1 })
    mockFetchGenerationQueue.mockResolvedValue({
      total: 2,
      items: [{ ...RESPONSE.items[0], taskId: 'task-2', position: 2 }],
    })

    await act(async () => useGenerationQueueStore.getState().loadMore())

    expect(mockFetchGenerationQueue).toHaveBeenCalledWith(40, 1, 'image')
    expect(useGenerationQueueStore.getState().items).toHaveLength(2)
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

it('切换队列后丢弃旧列表响应，包括切回原类型的情况', async () => {
  let resolveOld!: (value: typeof RESPONSE) => void
  mockFetchGenerationQueue.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        resolveOld = resolve
      })
  )
  const old = useGenerationQueueStore.getState().load()
  mockFetchGenerationQueue.mockResolvedValue({ total: 0, items: [] })
  await useGenerationQueueStore.getState().setTaskKind('text')
  expect(mockFetchGenerationQueue).toHaveBeenLastCalledWith(40, 0, 'text')
  await useGenerationQueueStore.getState().setTaskKind('image')
  resolveOld(RESPONSE)
  await old
  expect(useGenerationQueueStore.getState().items).toEqual([])
  expect(useGenerationQueueStore.getState().loading).toBe(false)
})

it('文本队列分页和操作后刷新都保留类型筛选', async () => {
  mockFetchGenerationQueue.mockResolvedValue({ total: 0, items: [] })
  await useGenerationQueueStore.getState().setTaskKind('text')
  useGenerationQueueStore.setState({ hasMore: true, offset: 40 })
  await useGenerationQueueStore.getState().loadMore()
  expect(mockFetchGenerationQueue).toHaveBeenLastCalledWith(40, 40, 'text')
  await useGenerationQueueStore.getState().runAction('cancel', ['text-1'])
  expect(mockRunGenerationQueueAction).toHaveBeenCalledWith('cancel', ['text-1'])
  expect(mockFetchGenerationQueue).toHaveBeenLastCalledWith(40, 0, 'text')
})

it('切换队列时取消旧选择对应的列表，不允许旧分页混入', async () => {
  let resolveMore!: (value: typeof RESPONSE) => void
  useGenerationQueueStore.setState({ items: RESPONSE.items, offset: 1, hasMore: true })
  mockFetchGenerationQueue.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        resolveMore = resolve
      })
  )
  const loadingMore = useGenerationQueueStore.getState().loadMore()
  mockFetchGenerationQueue.mockResolvedValue({ total: 0, items: [] })
  await useGenerationQueueStore.getState().setTaskKind('text')
  resolveMore(RESPONSE)
  await loadingMore
  expect(useGenerationQueueStore.getState().items).toEqual([])
  expect(useGenerationQueueStore.getState().loadingMore).toBe(false)
})

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act } from '@testing-library/react'
import { useTaskStore } from '../useTaskStore'
import type { Task } from '../../types/task'

const mockFetchTasks = vi.fn()
const mockCancelTask = vi.fn()

vi.mock('../../services/task-service', () => ({
  fetchTasks: (...args: unknown[]) => mockFetchTasks(...args),
  cancelTask: (...args: unknown[]) => mockCancelTask(...args),
}))

const SAMPLE_TASKS: Task[] = [
  {
    id: 'task-001',
    platform: 'doubao',
    task_kind: 'image',
    model_id: 'doubao-xl',
    prompt: '测试',
    status: 'processing',
    created_at: '2026-04-13T10:00:00Z',
  },
  {
    id: 'task-002',
    platform: 'chatgpt',
    task_kind: 'video',
    model_id: 'ChatGPT',
    prompt: '测试2',
    status: 'completed',
    created_at: '2026-04-13T10:05:00Z',
  },
]

beforeEach(() => {
  mockFetchTasks.mockReset()
  mockCancelTask.mockReset()
  // 重置 store 状态
  useTaskStore.setState({
    tasks: [],
    filters: {},
    selectedTaskId: null,
    activePlatform: '',
    loading: false,
    loadingMore: false,
    hasMore: true,
    nextOffset: 0,
    error: '',
  })
})

describe('useTaskStore', () => {
  describe('初始状态', () => {
    it('初始值正确', () => {
      const state = useTaskStore.getState()
      expect(state.tasks).toEqual([])
      expect(state.selectedTaskId).toBeNull()
      expect(state.activePlatform).toBe('')
      expect(state.loading).toBe(false)
      expect(state.error).toBe('')
    })
  })

  describe('fetch', () => {
    it('获取任务列表并更新 tasks', async () => {
      mockFetchTasks.mockResolvedValue(SAMPLE_TASKS)

      await act(async () => {
        await useTaskStore.getState().fetch()
      })

      const state = useTaskStore.getState()
      expect(state.tasks).toEqual(SAMPLE_TASKS)
      expect(state.loading).toBe(false)
    })

    it('使用指定 platform 获取', async () => {
      mockFetchTasks.mockResolvedValue([SAMPLE_TASKS[0]])

      await act(async () => {
        await useTaskStore.getState().fetch('doubao')
      })

      expect(mockFetchTasks).toHaveBeenCalledWith('doubao', 40, 0, {})
      expect(useTaskStore.getState().tasks).toHaveLength(1)
    })

    it('未指定 platform 时使用 activePlatform', async () => {
      mockFetchTasks.mockResolvedValue([])
      useTaskStore.setState({ activePlatform: 'chatgpt' })

      await act(async () => {
        await useTaskStore.getState().fetch()
      })

      expect(mockFetchTasks).toHaveBeenCalledWith('chatgpt', 40, 0, {})
    })

    it('fetch 失败时 loading 恢复为 false', async () => {
      mockFetchTasks.mockRejectedValue(new Error('网络错误'))

      await act(async () => {
        try {
          await useTaskStore.getState().fetch()
        } catch {
          // 预期抛错
        }
      })

      expect(useTaskStore.getState().loading).toBe(false)
    })

    it('fetch 过程中 loading 为 true', async () => {
      let resolvePromise: (value: Task[]) => void
      mockFetchTasks.mockReturnValue(
        new Promise<Task[]>((resolve) => {
          resolvePromise = resolve
        })
      )

      let fetchPromise: Promise<void>
      act(() => {
        fetchPromise = useTaskStore.getState().fetch()
      })

      expect(useTaskStore.getState().loading).toBe(true)

      await act(async () => {
        resolvePromise!(SAMPLE_TASKS)
        await fetchPromise!
      })

      expect(useTaskStore.getState().loading).toBe(false)
    })
  })

  describe('setActivePlatform', () => {
    it('设置 activePlatform 并清除 selectedTaskId', async () => {
      mockFetchTasks.mockResolvedValue([])
      useTaskStore.setState({ selectedTaskId: 'task-001' })

      await act(async () => {
        useTaskStore.getState().setActivePlatform('chatgpt')
      })

      const state = useTaskStore.getState()
      expect(state.activePlatform).toBe('chatgpt')
      expect(state.selectedTaskId).toBeNull()
    })

    it('切换平台后触发 fetch', async () => {
      mockFetchTasks.mockResolvedValue([])

      await act(async () => {
        useTaskStore.getState().setActivePlatform('doubao')
      })

      expect(mockFetchTasks).toHaveBeenCalledWith('doubao', 40, 0, {})
    })
  })

  it('接近底部时从当前任务数量继续加载', async () => {
    useTaskStore.setState({
      tasks: [SAMPLE_TASKS[0]],
      activePlatform: 'doubao',
      hasMore: true,
      nextOffset: 1,
    })
    mockFetchTasks.mockResolvedValue([SAMPLE_TASKS[1]])

    await act(async () => useTaskStore.getState().loadMore())

    expect(mockFetchTasks).toHaveBeenCalledWith('doubao', 40, 1, {})
    expect(useTaskStore.getState().tasks).toEqual(SAMPLE_TASKS)
  })

  it('组合筛选重置分页、保留刷新和后续页的条件，切换平台清空筛选', async () => {
    const filters = {
      task_kind: 'image' as const,
      status: 'failed' as const,
      account_id: 'acc',
      model_id: 'model',
    }
    const page = Array.from({ length: 40 }, (_, i) => ({ ...SAMPLE_TASKS[0], id: String(i) }))
    mockFetchTasks.mockResolvedValue(page)
    useTaskStore.setState({ activePlatform: 'doubao', nextOffset: 80, selectedTaskId: 'old' })
    await act(async () => useTaskStore.getState().setFilters(filters))
    expect(mockFetchTasks).toHaveBeenLastCalledWith('doubao', 40, 0, filters)
    expect(useTaskStore.getState().selectedTaskId).toBeNull()
    await act(async () => useTaskStore.getState().loadMore())
    expect(mockFetchTasks).toHaveBeenLastCalledWith('doubao', 40, 40, filters)
    await act(async () => useTaskStore.getState().fetch())
    expect(mockFetchTasks).toHaveBeenLastCalledWith('doubao', 40, 0, filters)
    await act(async () => useTaskStore.getState().setActivePlatform('chatgpt'))
    expect(useTaskStore.getState().filters).toEqual({})
    expect(mockFetchTasks).toHaveBeenLastCalledWith('chatgpt', 40, 0, {})
  })

  it('快速切换筛选时忽略旧请求结果和错误', async () => {
    let resolveOld!: (tasks: Task[]) => void
    let rejectOld!: (error: Error) => void
    mockFetchTasks.mockImplementationOnce(
      () =>
        new Promise<Task[]>((resolve) => {
          resolveOld = resolve
        })
    )
    const first = useTaskStore.getState().fetch()
    mockFetchTasks.mockResolvedValueOnce([SAMPLE_TASKS[1]])
    await act(async () => useTaskStore.getState().setFilters({ task_kind: 'video' }))
    resolveOld([SAMPLE_TASKS[0]])
    await first
    expect(useTaskStore.getState().tasks).toEqual([SAMPLE_TASKS[1]])
    mockFetchTasks.mockImplementationOnce(
      () =>
        new Promise<Task[]>((_, reject) => {
          rejectOld = reject
        })
    )
    const old = useTaskStore.getState().fetch()
    mockFetchTasks.mockResolvedValueOnce([])
    await act(async () => useTaskStore.getState().setFilters({ status: 'failed' }))
    rejectOld(new Error('旧请求失败'))
    await old
    expect(useTaskStore.getState().error).toBe('')
    expect(useTaskStore.getState().loading).toBe(false)
  })

  it('切换筛选后不追加旧分页结果', async () => {
    let resolvePage!: (tasks: Task[]) => void
    useTaskStore.setState({ tasks: [SAMPLE_TASKS[0]], nextOffset: 40 })
    mockFetchTasks.mockImplementationOnce(
      () =>
        new Promise<Task[]>((resolve) => {
          resolvePage = resolve
        })
    )
    const more = useTaskStore.getState().loadMore()
    mockFetchTasks.mockResolvedValueOnce([SAMPLE_TASKS[1]])
    await act(async () => useTaskStore.getState().setFilters({ task_kind: 'video' }))
    resolvePage([SAMPLE_TASKS[0]])
    await more
    expect(useTaskStore.getState().tasks).toEqual([SAMPLE_TASKS[1]])
    expect(useTaskStore.getState().nextOffset).toBe(1)
    expect(useTaskStore.getState().loadingMore).toBe(false)
  })

  it('按状态筛选时取消成功后重新查询，移除不再匹配的任务', async () => {
    useTaskStore.setState({ tasks: [SAMPLE_TASKS[0]], filters: { status: 'processing' } })
    mockCancelTask.mockResolvedValue({ ...SAMPLE_TASKS[0], status: 'cancelled' })
    mockFetchTasks.mockResolvedValue([])
    await useTaskStore.getState().cancel('task-001')
    expect(useTaskStore.getState().tasks).toEqual([])
    expect(mockFetchTasks).toHaveBeenCalledWith('', 40, 0, { status: 'processing' })
  })

  describe('selectTask', () => {
    it('设置选中任务 ID', () => {
      act(() => {
        useTaskStore.getState().selectTask('task-001')
      })
      expect(useTaskStore.getState().selectedTaskId).toBe('task-001')
    })

    it('传 null 取消选中', () => {
      useTaskStore.setState({ selectedTaskId: 'task-001' })
      act(() => {
        useTaskStore.getState().selectTask(null)
      })
      expect(useTaskStore.getState().selectedTaskId).toBeNull()
    })
  })

  describe('cancel', () => {
    it('取消任务后状态变为 cancelled 并附带错误信息', async () => {
      mockCancelTask.mockResolvedValue(undefined)
      useTaskStore.setState({ tasks: SAMPLE_TASKS })

      await act(async () => {
        await useTaskStore.getState().cancel('task-001')
      })

      const task = useTaskStore.getState().tasks.find((t) => t.id === 'task-001')
      expect(task?.status).toBe('cancelled')
      expect(task?.error_message).toBe('用户取消')
    })

    it('取消任务不影响其他任务', async () => {
      mockCancelTask.mockResolvedValue(undefined)
      useTaskStore.setState({ tasks: SAMPLE_TASKS })

      await act(async () => {
        await useTaskStore.getState().cancel('task-001')
      })

      const otherTask = useTaskStore.getState().tasks.find((t) => t.id === 'task-002')
      expect(otherTask?.status).toBe('completed')
    })

    it('调用 cancelTask service', async () => {
      mockCancelTask.mockResolvedValue(undefined)
      useTaskStore.setState({ tasks: SAMPLE_TASKS })

      await act(async () => {
        await useTaskStore.getState().cancel('task-001')
      })

      expect(mockCancelTask).toHaveBeenCalledWith('task-001')
    })
  })
})

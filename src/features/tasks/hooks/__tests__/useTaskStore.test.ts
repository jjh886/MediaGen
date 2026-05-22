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
    platform: 'flow',
    task_kind: 'video',
    model_id: 'flow-gen2',
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
    selectedTaskId: null,
    activePlatform: '',
    loading: false,
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

      expect(mockFetchTasks).toHaveBeenCalledWith('doubao')
      expect(useTaskStore.getState().tasks).toHaveLength(1)
    })

    it('未指定 platform 时使用 activePlatform', async () => {
      mockFetchTasks.mockResolvedValue([])
      useTaskStore.setState({ activePlatform: 'flow' })

      await act(async () => {
        await useTaskStore.getState().fetch()
      })

      expect(mockFetchTasks).toHaveBeenCalledWith('flow')
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
        useTaskStore.getState().setActivePlatform('flow')
      })

      const state = useTaskStore.getState()
      expect(state.activePlatform).toBe('flow')
      expect(state.selectedTaskId).toBeNull()
    })

    it('切换平台后触发 fetch', async () => {
      mockFetchTasks.mockResolvedValue([])

      await act(async () => {
        useTaskStore.getState().setActivePlatform('doubao')
      })

      expect(mockFetchTasks).toHaveBeenCalledWith('doubao')
    })
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
    it('取消任务后状态变为 failed 并附带错误信息', async () => {
      mockCancelTask.mockResolvedValue(undefined)
      useTaskStore.setState({ tasks: SAMPLE_TASKS })

      await act(async () => {
        await useTaskStore.getState().cancel('task-001')
      })

      const task = useTaskStore.getState().tasks.find((t) => t.id === 'task-001')
      expect(task?.status).toBe('failed')
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

import { create } from 'zustand'
import type { Task } from '../types/task'
import { fetchTasks, cancelTask } from '../services/task-service'
import {
  appendUnique,
  listErrorMessage,
  LIST_PAGE_SIZE,
  runPageRequest,
} from '@/shared/services/list-pagination'

interface TaskStore {
  tasks: Task[]
  selectedTaskId: string | null
  activePlatform: string
  loading: boolean
  loadingMore: boolean
  hasMore: boolean
  nextOffset: number
  error: string
  fetch: (platform?: string) => Promise<void>
  loadMore: () => Promise<void>
  setActivePlatform: (platform: string) => void
  selectTask: (id: string | null) => void
  cancel: (taskId: string) => Promise<void>
}

export const useTaskStore = create<TaskStore>((set, get) => ({
  tasks: [],
  selectedTaskId: null,
  activePlatform: '',
  loading: false,
  loadingMore: false,
  hasMore: true,
  nextOffset: 0,
  error: '',

  fetch: async (platform?: string) => {
    const p = platform ?? get().activePlatform
    set({ error: '' })
    const tasks = await runPageRequest(
      (loading) => set({ loading }),
      () => fetchTasks(p, LIST_PAGE_SIZE, 0),
      (error) => set({ error: listErrorMessage(error, '任务加载失败。') })
    )
    set({ tasks, hasMore: tasks.length === LIST_PAGE_SIZE, nextOffset: tasks.length })
  },

  loadMore: async () => {
    const { activePlatform, hasMore, loadingMore, nextOffset } = get()
    if (!hasMore || loadingMore) return
    set({ error: '' })
    const page = await runPageRequest(
      (loadingMore) => set({ loadingMore }),
      () => fetchTasks(activePlatform, LIST_PAGE_SIZE, nextOffset),
      (error) => set({ error: listErrorMessage(error, '任务加载失败。') })
    )
    set((state) => ({
      tasks: appendUnique(state.tasks, page, (task) => task.id),
      hasMore: page.length === LIST_PAGE_SIZE,
      nextOffset: state.nextOffset + page.length,
    }))
  },

  setActivePlatform: (platform) => {
    set({ activePlatform: platform, selectedTaskId: null })
    void get()
      .fetch(platform)
      .catch(() => undefined)
  },

  selectTask: (id) => set({ selectedTaskId: id }),

  cancel: async (taskId) => {
    const task = await cancelTask(taskId)
    set((s) => ({
      tasks: s.tasks.map((item) =>
        item.id === taskId
          ? task || { ...item, status: 'cancelled', error_message: '用户取消' }
          : item
      ),
    }))
  },
}))

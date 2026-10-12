import { create } from 'zustand'
import type { Task, TaskFilters } from '../types/task'
import { fetchTasks, cancelTask } from '../services/task-service'
import { appendUnique, listErrorMessage, LIST_PAGE_SIZE } from '@/shared/services/list-pagination'

interface TaskStore {
  tasks: Task[]
  filters: TaskFilters
  selectedTaskId: string | null
  activePlatform: string
  loading: boolean
  loadingMore: boolean
  hasMore: boolean
  nextOffset: number
  error: string
  fetch: (platform?: string) => Promise<void>
  loadMore: () => Promise<void>
  setFilters: (filters: TaskFilters) => void
  setActivePlatform: (platform: string) => void
  selectTask: (id: string | null) => void
  cancel: (taskId: string) => Promise<void>
}

let requestVersion = 0

export const useTaskStore = create<TaskStore>((set, get) => ({
  tasks: [],
  filters: {},
  selectedTaskId: null,
  activePlatform: '',
  loading: false,
  loadingMore: false,
  hasMore: true,
  nextOffset: 0,
  error: '',

  fetch: async (platform?: string) => {
    const version = ++requestVersion
    const p = platform ?? get().activePlatform
    const filters = get().filters
    set({ error: '', loading: true, loadingMore: false })
    try {
      const tasks = await fetchTasks(p, LIST_PAGE_SIZE, 0, filters)
      if (version !== requestVersion) return
      set({ tasks, hasMore: tasks.length === LIST_PAGE_SIZE, nextOffset: tasks.length })
    } catch (error) {
      if (version !== requestVersion) return
      set({ error: listErrorMessage(error, '任务加载失败。') })
      throw error
    } finally {
      if (version === requestVersion) set({ loading: false })
    }
  },

  loadMore: async () => {
    const { activePlatform, filters, hasMore, loading, loadingMore, nextOffset } = get()
    if (!hasMore || loading || loadingMore) return
    const version = requestVersion
    set({ error: '', loadingMore: true })
    try {
      const page = await fetchTasks(activePlatform, LIST_PAGE_SIZE, nextOffset, filters)
      if (version !== requestVersion) return
      set((state) => ({
        tasks: appendUnique(state.tasks, page, (task) => task.id),
        hasMore: page.length === LIST_PAGE_SIZE,
        nextOffset: nextOffset + page.length,
      }))
    } catch (error) {
      if (version !== requestVersion) return
      set({ error: listErrorMessage(error, '任务加载失败。') })
      throw error
    } finally {
      if (version === requestVersion) set({ loadingMore: false })
    }
  },

  setFilters: (filters) => {
    set({ filters, tasks: [], selectedTaskId: null, nextOffset: 0, hasMore: true })
    void get()
      .fetch()
      .catch(() => undefined)
  },

  setActivePlatform: (platform) => {
    set({
      activePlatform: platform,
      filters: {},
      tasks: [],
      selectedTaskId: null,
      nextOffset: 0,
      hasMore: true,
    })
    void get()
      .fetch(platform)
      .catch(() => undefined)
  },

  selectTask: (id) => set({ selectedTaskId: id }),

  cancel: async (taskId) => {
    const task = await cancelTask(taskId)
    if (get().filters.status) {
      await get().fetch()
      return
    }
    set((s) => ({
      tasks: s.tasks.map((item) =>
        item.id === taskId
          ? task || { ...item, status: 'cancelled', error_message: '用户取消' }
          : item
      ),
    }))
  },
}))

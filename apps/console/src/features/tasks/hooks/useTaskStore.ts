import { create } from 'zustand'
import type { Task } from '../types/task'
import { fetchTasks, cancelTask } from '../services/task-service'

interface TaskStore {
  tasks: Task[]
  selectedTaskId: string | null
  activePlatform: string
  loading: boolean
  error: string
  fetch: (platform?: string) => Promise<void>
  setActivePlatform: (platform: string) => void
  selectTask: (id: string | null) => void
  cancel: (taskId: string) => Promise<void>
}

export const useTaskStore = create<TaskStore>((set, get) => ({
  tasks: [],
  selectedTaskId: null,
  activePlatform: '',
  loading: false,
  error: '',

  fetch: async (platform?: string) => {
    const p = platform ?? get().activePlatform
    set({ loading: true, error: '' })
    try {
      const tasks = await fetchTasks(p)
      set({ tasks })
    } catch (error) {
      set({ error: error instanceof Error ? error.message : '任务加载失败。' })
      throw error
    } finally {
      set({ loading: false })
    }
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

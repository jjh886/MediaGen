import { create } from 'zustand'
import type { Task } from '../types/task'
import { fetchTasks, cancelTask } from '../services/task-service'

interface TaskStore {
  tasks: Task[]
  selectedTaskId: string | null
  activePlatform: string
  loading: boolean
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

  fetch: async (platform?: string) => {
    const p = platform ?? get().activePlatform
    set({ loading: true })
    try {
      const tasks = await fetchTasks(p)
      set({ tasks })
    } finally {
      set({ loading: false })
    }
  },

  setActivePlatform: (platform) => {
    set({ activePlatform: platform, selectedTaskId: null })
    get().fetch(platform)
  },

  selectTask: (id) => set({ selectedTaskId: id }),

  cancel: async (taskId) => {
    await cancelTask(taskId)
    set((s) => ({
      tasks: s.tasks.map((t) =>
        t.id === taskId ? { ...t, status: 'failed' as const, error_message: '用户取消' } : t
      ),
    }))
  },
}))

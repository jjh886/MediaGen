import { create } from 'zustand'
import { fetchLogs, type LogEntry, type LogQuery } from '../services/log-service'

export interface LogState {
  logs: LogEntry[]
  loading: boolean
  error: string
  moduleFilter: string
  levelFilter: string
  keyword: string
  taskId: string
  expandedId: string | null

  setModuleFilter: (v: string) => void
  setLevelFilter: (v: string) => void
  setKeyword: (v: string) => void
  setTaskId: (v: string) => void
  toggleExpand: (id: string) => void
  load: () => Promise<void>
}

export const useLogStore = create<LogState>((set, get) => ({
  logs: [],
  loading: false,
  error: '',
  moduleFilter: '',
  levelFilter: '',
  keyword: '',
  taskId: '',
  expandedId: null,

  setModuleFilter: (v) => {
    set({ moduleFilter: v })
    get().load()
  },
  setLevelFilter: (v) => {
    set({ levelFilter: v })
    get().load()
  },
  setKeyword: (v) => set({ keyword: v }),
  setTaskId: (v) => set({ taskId: v }),

  toggleExpand: (id) => set((s) => ({ expandedId: s.expandedId === id ? null : id })),

  load: async () => {
    set({ loading: true, error: '' })
    const { moduleFilter, levelFilter, keyword, taskId } = get()
    const query: LogQuery = {}
    if (moduleFilter) query.module = moduleFilter
    if (levelFilter) query.level = levelFilter
    if (keyword) query.keyword = keyword
    if (taskId) query.task_id = taskId
    try {
      const logs = await fetchLogs(query)
      set({ logs, loading: false })
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : '日志加载失败。',
        loading: false,
      })
    }
  },
}))

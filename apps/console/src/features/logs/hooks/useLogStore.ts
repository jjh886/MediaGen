import { create } from 'zustand'
import { fetchLogs, type LogEntry, type LogQuery } from '../services/log-service'

interface LogState {
  logs: LogEntry[]
  loading: boolean
  moduleFilter: string
  levelFilter: string
  keyword: string
  expandedId: string | null

  setModuleFilter: (v: string) => void
  setLevelFilter: (v: string) => void
  setKeyword: (v: string) => void
  toggleExpand: (id: string) => void
  load: () => Promise<void>
}

export const useLogStore = create<LogState>((set, get) => ({
  logs: [],
  loading: false,
  moduleFilter: '',
  levelFilter: '',
  keyword: '',
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

  toggleExpand: (id) => set((s) => ({ expandedId: s.expandedId === id ? null : id })),

  load: async () => {
    set({ loading: true })
    const { moduleFilter, levelFilter, keyword } = get()
    const query: LogQuery = {}
    if (moduleFilter) query.module = moduleFilter
    if (levelFilter) query.level = levelFilter
    if (keyword) query.keyword = keyword
    const logs = await fetchLogs(query)
    set({ logs, loading: false })
  },
}))

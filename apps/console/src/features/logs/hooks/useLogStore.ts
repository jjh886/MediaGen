import { create } from 'zustand'
import { fetchLogs, type LogEntry, type LogQuery } from '../services/log-service'
import { appendUnique, LIST_PAGE_SIZE } from '@/shared/services/list-pagination'

export interface LogState {
  logs: LogEntry[]
  loading: boolean
  loadingMore: boolean
  hasMore: boolean
  nextOffset: number
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
  loadMore: () => Promise<void>
  refreshLatest: () => Promise<void>
}

function currentQuery(state: LogState, offset = 0): LogQuery {
  const query: LogQuery = { limit: LIST_PAGE_SIZE, offset }
  if (state.moduleFilter) query.module = state.moduleFilter
  if (state.levelFilter) query.level = state.levelFilter
  if (state.keyword) query.keyword = state.keyword
  if (state.taskId) query.task_id = state.taskId
  return query
}

export const useLogStore = create<LogState>((set, get) => ({
  logs: [],
  loading: false,
  loadingMore: false,
  hasMore: true,
  nextOffset: 0,
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
    try {
      const logs = await fetchLogs(currentQuery(get()))
      set({
        logs,
        hasMore: logs.length === LIST_PAGE_SIZE,
        nextOffset: logs.length,
        loading: false,
      })
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : '日志加载失败。',
        loading: false,
      })
    }
  },

  loadMore: async () => {
    const state = get()
    if (!state.hasMore || state.loadingMore) return
    set({ loadingMore: true, error: '' })
    try {
      const page = await fetchLogs(currentQuery(state, state.nextOffset))
      set((current) => ({
        logs: appendUnique(current.logs, page, (log) => log.id),
        hasMore: page.length === LIST_PAGE_SIZE,
        nextOffset: current.nextOffset + page.length,
      }))
    } catch (error) {
      set({ error: error instanceof Error ? error.message : '日志加载失败。' })
    } finally {
      set({ loadingMore: false })
    }
  },

  refreshLatest: async () => {
    const state = get()
    // 用户已向下浏览后暂停自动改动列表，避免新日志插入顶部造成阅读位置跳动。
    if (state.logs.length > LIST_PAGE_SIZE || state.loading || state.loadingMore) return
    try {
      const latest = await fetchLogs(currentQuery(state))
      set({
        logs: latest,
        hasMore: latest.length === LIST_PAGE_SIZE,
        nextOffset: latest.length,
      })
    } catch {
      // 自动刷新失败不覆盖当前列表，下一轮会继续尝试。
    }
  },
}))

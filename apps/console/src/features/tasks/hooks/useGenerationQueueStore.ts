import { create } from 'zustand'
import {
  fetchGenerationQueue,
  runGenerationQueueAction,
} from '../services/generation-queue-service'
import type {
  GenerationQueueAction,
  GenerationQueueItem,
  GenerationQueueKind,
} from '../types/generation-queue'
import { appendUnique, LIST_PAGE_SIZE } from '@/shared/services/list-pagination'

interface GenerationQueueStore {
  taskKind: GenerationQueueKind
  revision: number
  setTaskKind: (kind: GenerationQueueKind) => Promise<void>
  items: GenerationQueueItem[]
  total: number
  limit: number
  offset: number
  loading: boolean
  loadingMore: boolean
  hasMore: boolean
  error: string
  load: (limit?: number, offset?: number) => Promise<void>
  loadMore: () => Promise<void>
  refreshLatest: () => Promise<void>
  runAction: (action: GenerationQueueAction, taskIds: string[]) => Promise<void>
}

function errorMessage(error: unknown): string {
  return error instanceof Error && error.message.trim() ? error.message : '生成队列操作失败。'
}

export const useGenerationQueueStore = create<GenerationQueueStore>((set, get) => ({
  taskKind: 'image',
  revision: 0,
  setTaskKind: async (taskKind) => {
    if (get().taskKind === taskKind) return
    set({ taskKind, items: [], total: 0, offset: 0, hasMore: true, loadingMore: false })
    await get().load()
  },
  items: [],
  total: 0,
  limit: LIST_PAGE_SIZE,
  offset: 0,
  loading: false,
  loadingMore: false,
  hasMore: true,
  error: '',

  load: async (limit = LIST_PAGE_SIZE, offset = 0) => {
    const revision = get().revision + 1
    const taskKind = get().taskKind
    set({ loading: true, error: '', limit, offset, revision })
    try {
      const response = await fetchGenerationQueue(limit, offset, taskKind)
      if (get().revision !== revision) return
      set({
        items: response.items,
        total: response.total,
        hasMore: response.items.length < response.total,
        offset: offset + response.items.length,
      })
    } catch (error) {
      if (get().revision !== revision) return
      set({ error: errorMessage(error) })
      throw error
    } finally {
      if (get().revision === revision) set({ loading: false })
    }
  },

  loadMore: async () => {
    const state = get()
    if (!state.hasMore || state.loading || state.loadingMore) return
    set({ loadingMore: true, error: '' })
    try {
      const response = await fetchGenerationQueue(LIST_PAGE_SIZE, state.offset, state.taskKind)
      if (get().revision !== state.revision) return
      set((current) => {
        const items = appendUnique(current.items, response.items, (item) => item.taskId)
        return {
          items,
          total: response.total,
          hasMore: items.length < response.total,
          offset: current.offset + response.items.length,
        }
      })
    } catch (error) {
      if (get().revision !== state.revision) return
      set({ error: errorMessage(error) })
      throw error
    } finally {
      if (get().revision === state.revision) set({ loadingMore: false })
    }
  },

  refreshLatest: async () => {
    const state = get()
    // 已向下浏览时不自动改动队列位置，避免正在操作的行突然跳走。
    if (state.items.length > LIST_PAGE_SIZE || state.loading || state.loadingMore) return
    const response = await fetchGenerationQueue(LIST_PAGE_SIZE, 0, state.taskKind)
    if (get().revision !== state.revision) return
    set({
      items: response.items,
      total: response.total,
      hasMore: response.items.length < response.total,
      offset: response.items.length,
    })
  },

  runAction: async (action, taskIds) => {
    if (taskIds.length === 0) return
    const { revision } = get()
    set({ error: '' })
    try {
      await runGenerationQueueAction(action, taskIds)
      if (get().revision === revision) await get().load(LIST_PAGE_SIZE, 0)
    } catch (error) {
      if (get().revision === revision) set({ error: errorMessage(error) })
      throw error
    }
  },
}))

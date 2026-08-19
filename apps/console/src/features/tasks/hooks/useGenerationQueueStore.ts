import { create } from 'zustand'
import {
  fetchGenerationQueue,
  runGenerationQueueAction,
} from '../services/generation-queue-service'
import type { GenerationQueueAction, GenerationQueueItem } from '../types/generation-queue'
import { appendUnique, LIST_PAGE_SIZE } from '@/shared/services/list-pagination'

interface GenerationQueueStore {
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
  items: [],
  total: 0,
  limit: LIST_PAGE_SIZE,
  offset: 0,
  loading: false,
  loadingMore: false,
  hasMore: true,
  error: '',

  load: async (limit = LIST_PAGE_SIZE, offset = 0) => {
    set({ loading: true, error: '', limit, offset })
    try {
      const response = await fetchGenerationQueue(limit, offset)
      set({
        items: response.items,
        total: response.total,
        hasMore: response.items.length < response.total,
        offset: offset + response.items.length,
      })
    } catch (error) {
      set({ error: errorMessage(error) })
      throw error
    } finally {
      set({ loading: false })
    }
  },

  loadMore: async () => {
    const state = get()
    if (!state.hasMore || state.loadingMore) return
    set({ loadingMore: true, error: '' })
    try {
      const response = await fetchGenerationQueue(LIST_PAGE_SIZE, state.offset)
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
      set({ error: errorMessage(error) })
      throw error
    } finally {
      set({ loadingMore: false })
    }
  },

  refreshLatest: async () => {
    const state = get()
    // 已向下浏览时不自动改动队列位置，避免正在操作的行突然跳走。
    if (state.items.length > LIST_PAGE_SIZE || state.loading || state.loadingMore) return
    const response = await fetchGenerationQueue(LIST_PAGE_SIZE, 0)
    set({
      items: response.items,
      total: response.total,
      hasMore: response.items.length < response.total,
      offset: response.items.length,
    })
  },

  runAction: async (action, taskIds) => {
    if (taskIds.length === 0) return
    set({ error: '' })
    try {
      await runGenerationQueueAction(action, taskIds)
      await get().load(LIST_PAGE_SIZE, 0)
    } catch (error) {
      set({ error: errorMessage(error) })
      throw error
    }
  },
}))

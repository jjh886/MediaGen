import { create } from 'zustand'
import {
  fetchGenerationQueue,
  runGenerationQueueAction,
} from '../services/generation-queue-service'
import type { GenerationQueueAction, GenerationQueueItem } from '../types/generation-queue'

interface GenerationQueueStore {
  items: GenerationQueueItem[]
  total: number
  limit: number
  offset: number
  loading: boolean
  error: string
  load: (limit?: number, offset?: number) => Promise<void>
  runAction: (action: GenerationQueueAction, taskIds: string[]) => Promise<void>
}

function errorMessage(error: unknown): string {
  return error instanceof Error && error.message.trim() ? error.message : '生成队列操作失败。'
}

export const useGenerationQueueStore = create<GenerationQueueStore>((set, get) => ({
  items: [],
  total: 0,
  limit: 100,
  offset: 0,
  loading: false,
  error: '',

  load: async (limit = get().limit, offset = get().offset) => {
    set({ loading: true, error: '', limit, offset })
    try {
      const response = await fetchGenerationQueue(limit, offset)
      set({ items: response.items, total: response.total })
    } catch (error) {
      set({ error: errorMessage(error) })
      throw error
    } finally {
      set({ loading: false })
    }
  },

  runAction: async (action, taskIds) => {
    if (taskIds.length === 0) return
    set({ error: '' })
    try {
      await runGenerationQueueAction(action, taskIds)
      await get().load()
    } catch (error) {
      set({ error: errorMessage(error) })
      throw error
    }
  },
}))

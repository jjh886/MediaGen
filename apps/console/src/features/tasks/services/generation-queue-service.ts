import { apiGet, apiPost } from '@/shared/services/http-api-client'
import type { GenerationQueueAction, GenerationQueueResponse } from '../types/generation-queue'

export async function fetchGenerationQueue(
  limit = 100,
  offset = 0
): Promise<GenerationQueueResponse> {
  const params = new URLSearchParams({ limit: String(limit), offset: String(offset) })
  return await apiGet<GenerationQueueResponse>(`/api/generation-queue?${params}`)
}

export async function runGenerationQueueAction(
  action: GenerationQueueAction,
  taskIds: string[]
): Promise<void> {
  await apiPost('/api/generation-queue/actions', { action, taskIds })
}

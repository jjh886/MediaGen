import { apiGet, apiPost } from '@/shared/services/http-api-client'
import { LIST_PAGE_SIZE } from '@/shared/services/list-pagination'
import type {
  GenerationQueueAction,
  GenerationQueueResponse,
  GenerationQueueKind,
} from '../types/generation-queue'

export async function fetchGenerationQueue(
  limit = LIST_PAGE_SIZE,
  offset = 0,
  taskKind?: GenerationQueueKind
): Promise<GenerationQueueResponse> {
  const params = new URLSearchParams({ limit: String(limit), offset: String(offset) })
  if (taskKind) params.set('taskKind', taskKind)
  return await apiGet<GenerationQueueResponse>(`/api/generation-queue?${params}`)
}

export async function runGenerationQueueAction(
  action: GenerationQueueAction,
  taskIds: string[]
): Promise<void> {
  await apiPost('/api/generation-queue/actions', { action, taskIds })
}

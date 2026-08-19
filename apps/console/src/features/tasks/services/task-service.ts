import { apiGet, apiPost } from '@/shared/services/http-api-client'
import { LIST_PAGE_SIZE, pagedQuery } from '@/shared/services/list-pagination'
import type { Task } from '../types/task'

export async function fetchTasks(
  platform = '',
  limit = LIST_PAGE_SIZE,
  offset = 0
): Promise<Task[]> {
  const params = pagedQuery(limit, offset)
  if (platform) params.set('platform', platform)
  return await apiGet<Task[]>(`/api/tasks?${params}`)
}

export async function cancelTask(taskId: string): Promise<Task> {
  return await apiPost<Task>(`/api/tasks/${encodeURIComponent(taskId)}/cancel`)
}

import { apiGet, apiPost } from '@/shared/services/http-api-client'
import type { Task } from '../types/task'

export async function fetchTasks(platform = '', limit = 50, offset = 0): Promise<Task[]> {
  const params = new URLSearchParams()
  if (platform) params.set('platform', platform)
  params.set('limit', String(limit))
  params.set('offset', String(offset))
  const query = params.toString()
  return await apiGet<Task[]>(`/api/tasks${query ? `?${query}` : ''}`)
}

export async function cancelTask(taskId: string): Promise<Task> {
  return await apiPost<Task>(`/api/tasks/${encodeURIComponent(taskId)}/cancel`)
}

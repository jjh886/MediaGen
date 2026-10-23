import { apiGet, apiPost } from '@/shared/services/http-api-client'
import { LIST_PAGE_SIZE, pagedQuery } from '@/shared/services/list-pagination'
import type { Task, TaskFilters, TaskFilterOptions } from '../types/task'

export async function fetchTasks(
  platform = '',
  limit = LIST_PAGE_SIZE,
  offset = 0,
  filters: TaskFilters = {}
): Promise<Task[]> {
  const params = pagedQuery(limit, offset)
  if (platform) params.set('platform', platform)
  for (const [key, value] of Object.entries(filters)) {
    if (value) params.set(key, value)
  }
  return await apiGet<Task[]>(`/api/tasks?${params}`)
}

export async function cancelTask(taskId: string): Promise<Task> {
  return await apiPost<Task>(`/api/tasks/${encodeURIComponent(taskId)}/cancel`)
}

export async function fetchTaskFilterOptions(platform: string): Promise<TaskFilterOptions> {
  const params = new URLSearchParams({ platform })
  return await apiGet<TaskFilterOptions>(`/api/tasks/filter-options?${params}`)
}

export async function fetchTaskResult(taskId: string): Promise<unknown> {
  return await apiGet<unknown>(`/api/tasks/${encodeURIComponent(taskId)}/result`)
}

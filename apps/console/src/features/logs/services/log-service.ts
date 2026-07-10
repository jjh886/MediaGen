import { apiGet, apiPost } from '@/shared/services/http-api-client'

export interface LogEntry {
  id: string
  timestamp: string
  module: 'engine' | 'scheduler' | 'account' | 'api' | 'registration'
  level: 'debug' | 'info' | 'warn' | 'error'
  summary: string
  detail?: string
  task_id?: string
}

export interface LogQuery {
  module?: string
  level?: string
  keyword?: string
  task_id?: string
  offset?: number
  limit?: number
}

export async function fetchLogs(query: LogQuery): Promise<LogEntry[]> {
  const params = new URLSearchParams()
  if (query.module) params.set('module', query.module)
  if (query.level) params.set('level', query.level)
  if (query.keyword) params.set('keyword', query.keyword)
  if (query.task_id) params.set('task_id', query.task_id)
  params.set('limit', String(query.limit ?? 200))
  params.set('offset', String(query.offset ?? 0))
  return await apiGet<LogEntry[]>(`/api/logs?${params.toString()}`)
}

export async function clearLogs(): Promise<void> {
  await apiPost('/api/logs/clear').catch(() => undefined)
}

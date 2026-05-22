import { tauriInvoke } from '@/shared/services/tauri-bridge'

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
  offset?: number
  limit?: number
}

export async function fetchLogs(query: LogQuery): Promise<LogEntry[]> {
  const result = await tauriInvoke<LogEntry[]>('get_logs', {
    module: query.module || null,
    level: query.level || null,
    keyword: query.keyword || null,
    limit: query.limit ?? 200,
    offset: query.offset ?? 0,
  })
  return result ?? []
}

export async function clearLogs(): Promise<void> {
  await tauriInvoke('clear_logs')
}

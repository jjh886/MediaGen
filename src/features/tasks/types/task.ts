export type TaskStatus = 'pending' | 'processing' | 'completed' | 'failed'
export type TaskKind = 'image' | 'video'

export interface TaskResultData {
  url?: string
  storage_key?: string
  width?: number
  height?: number
  source?: string
  imageCount?: number
}

export interface Task {
  id: string
  platform: string
  task_kind: TaskKind
  model_id: string
  prompt: string
  account_id?: string
  status: TaskStatus
  result_url?: string
  storage_key?: string
  error_message?: string
  result_data?: string
  created_at: string
  completed_at?: string
}

export const statusVariant: Record<TaskStatus, 'info' | 'warning' | 'success' | 'error'> = {
  pending: 'info',
  processing: 'warning',
  completed: 'success',
  failed: 'error',
}

export const statusLabel: Record<TaskStatus, string> = {
  pending: '等待中',
  processing: '生成中',
  completed: '已完成',
  failed: '失败',
}

/** 解析 result_data JSON 字符串 */
export function parseResultData(task: Task): TaskResultData | null {
  if (!task.result_data) return null
  try {
    return JSON.parse(task.result_data) as TaskResultData
  } catch {
    return null
  }
}

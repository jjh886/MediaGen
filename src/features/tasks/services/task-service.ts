import { tauriInvoke } from '@/shared/services/tauri-bridge'
import type { Task } from '../types/task'

const MOCK_TASKS: Task[] = [
  {
    id: 'task-002',
    platform: 'flow',
    task_kind: 'video',
    model_id: 'flow-gen2',
    prompt: '日落时分的海滩，海浪拍打沙滩',
    account_id: 'acc-003',
    status: 'processing',
    created_at: '2026-04-13T10:05:00Z',
  },
  {
    id: 'task-003',
    platform: 'doubao',
    task_kind: 'image',
    model_id: 'doubao-art',
    prompt: '中国山水画风格的未来城市',
    status: 'pending',
    created_at: '2026-04-13T10:10:00Z',
  },
]

const isTauri = '__TAURI_INTERNALS__' in window

export async function fetchTasks(platform = '', limit = 50, offset = 0): Promise<Task[]> {
  if (!isTauri) {
    if (!platform) return MOCK_TASKS
    return MOCK_TASKS.filter((t) => t.platform === platform)
  }
  const tasks =
    (await tauriInvoke<Task[]>('get_tasks', {
      platform: platform || null,
      limit,
      offset,
    })) ?? []
  return tasks
}

export async function cancelTask(taskId: string): Promise<void> {
  if (!isTauri) return
  await tauriInvoke('cancel_task', { taskId })
}

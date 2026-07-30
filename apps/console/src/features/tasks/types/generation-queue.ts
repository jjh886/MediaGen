export interface GenerationQueueItem {
  taskId: string
  platform: string
  modelId: string
  prompt: string
  status: 'queued'
  position: number
  queuedAt: string
}

export interface GenerationQueueResponse {
  total: number
  items: GenerationQueueItem[]
}

export type GenerationQueueAction = 'promote' | 'cancel'

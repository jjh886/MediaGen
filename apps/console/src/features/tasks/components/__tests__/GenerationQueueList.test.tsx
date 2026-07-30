import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import GenerationQueueList from '../GenerationQueueList'

const queueStore = vi.hoisted(() => ({
  items: [
    {
      taskId: 'task-1',
      platform: 'chatgpt',
      modelId: 'gpt-image-1',
      prompt: '雨夜街道',
      status: 'queued' as const,
      position: 1,
      queuedAt: '2026-07-18T10:00:00Z',
    },
  ],
  total: 1,
  loading: false,
  error: '',
  load: vi.fn(async () => undefined),
  runAction: vi.fn(async () => undefined),
}))

vi.mock('../../hooks/useGenerationQueueStore', () => ({
  useGenerationQueueStore: () => queueStore,
}))

describe('GenerationQueueList', () => {
  beforeEach(() => {
    queueStore.total = 1
    queueStore.load.mockClear()
    queueStore.runAction.mockClear()
  })

  it('确认后才执行取消操作', async () => {
    render(<GenerationQueueList />)
    fireEvent.click(screen.getByRole('button', { name: '取消' }))

    expect(screen.getByText('取消排队任务')).toBeInTheDocument()
    expect(queueStore.runAction).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: '确认取消' }))
    await waitFor(() => {
      expect(queueStore.runAction).toHaveBeenCalledWith('cancel', ['task-1'])
    })
  })

  it('返回后不执行取消操作', () => {
    render(<GenerationQueueList />)
    fireEvent.click(screen.getByRole('button', { name: '取消' }))
    fireEvent.click(screen.getByRole('button', { name: '返回' }))

    expect(screen.queryByText('取消排队任务')).not.toBeInTheDocument()
    expect(queueStore.runAction).not.toHaveBeenCalled()
  })

  it('按页加载并可切换到下一页', async () => {
    queueStore.total = 45
    render(<GenerationQueueList />)

    expect(queueStore.load).toHaveBeenCalledWith(20, 0)
    fireEvent.click(screen.getByRole('button', { name: '下一页' }))

    await waitFor(() => expect(queueStore.load).toHaveBeenCalledWith(20, 20))
    expect(screen.getByText('第 2 / 3 页')).toBeInTheDocument()
  })
})

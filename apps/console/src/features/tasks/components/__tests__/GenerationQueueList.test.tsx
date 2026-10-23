import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import GenerationQueueList from '../GenerationQueueList'

const queueStore = vi.hoisted(() => ({
  taskKind: 'image' as 'image' | 'text',
  setTaskKind: vi.fn(async () => undefined),
  items: [
    {
      taskKind: 'image' as const,
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
  loadingMore: false,
  hasMore: false,
  error: '',
  load: vi.fn(async () => undefined),
  loadMore: vi.fn(async () => undefined),
  refreshLatest: vi.fn(async () => undefined),
  runAction: vi.fn(async () => undefined),
}))

vi.mock('../../hooks/useGenerationQueueStore', () => ({
  useGenerationQueueStore: () => queueStore,
}))

describe('GenerationQueueList', () => {
  beforeEach(() => {
    queueStore.taskKind = 'image'
    queueStore.setTaskKind.mockClear()
    queueStore.total = 1
    queueStore.load.mockClear()
    queueStore.loadMore.mockClear()
    queueStore.refreshLatest.mockClear()
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

  it('默认加载最新一页并移除手动翻页按钮', async () => {
    queueStore.total = 45
    render(<GenerationQueueList />)

    expect(queueStore.load).toHaveBeenCalledWith()
    expect(screen.queryByRole('button', { name: '下一页' })).not.toBeInTheDocument()
  })
})

it('切换到文本队列时清空图片选择和取消弹窗', () => {
  Object.defineProperty(HTMLElement.prototype, 'scrollTo', { configurable: true, value: vi.fn() })
  render(<GenerationQueueList />)
  fireEvent.click(screen.getByRole('checkbox', { name: '选择任务 task-1' }))
  fireEvent.click(screen.getByRole('button', { name: '取消' }))
  fireEvent.click(screen.getByRole('tab', { name: '文本队列' }))
  expect(queueStore.setTaskKind).toHaveBeenCalledWith('text')
  expect(screen.queryByText('取消排队任务')).not.toBeInTheDocument()
  expect(screen.getByRole('checkbox')).not.toBeChecked()
  expect(screen.getByRole('button', { name: '批量取消' })).toBeDisabled()
})

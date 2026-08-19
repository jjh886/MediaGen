import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Task } from '../../types/task'

const cancel = vi.fn()
const fetch = vi.fn()
const setActivePlatform = vi.fn()
const selectTask = vi.fn()
const runningTask: Task = {
  id: 'task-running',
  platform: 'test',
  task_kind: 'image',
  model_id: 'model',
  prompt: '测试生成中取消',
  status: 'processing',
  created_at: '2026-07-20T10:00:00Z',
}

vi.mock('../../hooks/useTaskStore', () => ({
  useTaskStore: () => ({
    tasks: [runningTask],
    selectedTaskId: null,
    activePlatform: 'test',
    loading: false,
    loadingMore: false,
    hasMore: false,
    error: '',
    fetch,
    loadMore: vi.fn(),
    setActivePlatform,
    selectTask,
    cancel,
  }),
}))
vi.mock('@/shared/services/plugin-service', () => ({
  fetchPlugins: vi.fn().mockResolvedValue([]),
}))
vi.mock('@/shared/services/account-service', () => ({
  fetchAllAccounts: vi.fn().mockResolvedValue([]),
}))

import TaskList from '../TaskList'

describe('TaskList 取消生成中任务', () => {
  beforeEach(() => {
    cancel.mockReset()
    fetch.mockReset()
    setActivePlatform.mockReset()
    selectTask.mockReset()
  })

  it('确认后调用取消，失败时显示可读错误', async () => {
    cancel.mockRejectedValue(new Error('取消任务失败'))
    render(<TaskList />)

    fireEvent.click(screen.getByRole('button', { name: '取消' }))
    fireEvent.click(screen.getByRole('button', { name: '取消任务' }))

    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('取消任务失败'))
    expect(cancel).toHaveBeenCalledWith('task-running')
  })
})

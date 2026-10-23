import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Task, TaskFilters } from '../../types/task'

const setFilters = vi.fn()
let filters: TaskFilters = {}
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
    filters,
    setFilters,
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
  fetchAllAccounts: vi.fn().mockResolvedValue([{ id: 'acc-1', name: '测试账号' }]),
}))

vi.mock('../../services/task-service', () => ({
  fetchTaskResult: vi.fn(),
  fetchTaskFilterOptions: vi
    .fn()
    .mockResolvedValue({ account_ids: ['acc-1', 'deleted-account'], model_ids: ['old-model'] }),
}))

import TaskList from '../TaskList'
import TaskDetail from '../TaskDetail'
import { fetchTaskResult } from '../../services/task-service'

describe('TaskList 取消生成中任务', () => {
  beforeEach(() => {
    Object.defineProperty(HTMLElement.prototype, 'scrollTo', { configurable: true, value: vi.fn() })
    filters = {}
    setFilters.mockReset()
    fetch.mockResolvedValue(undefined)
    cancel.mockReset()
    fetch.mockReset().mockResolvedValue(undefined)
    setActivePlatform.mockReset()
    selectTask.mockReset()
  })

  it('刷新旁提供四个组合筛选，包含历史模型和已删除账号', async () => {
    filters = { status: 'completed' }
    render(<TaskList />)
    await screen.findByRole('option', { name: 'old-model' })
    expect(screen.getAllByRole('combobox')).toHaveLength(4)
    expect(screen.getByRole('group', { name: '任务筛选' })).toContainElement(
      screen.getByRole('button', { name: '刷新' })
    )
    expect(screen.getByRole('option', { name: 'deleted-account' })).toBeInTheDocument()
    for (const [name, value, key] of [
      ['任务类型', 'video', 'task_kind'],
      ['账号', 'acc-1', 'account_id'],
      ['模型', 'old-model', 'model_id'],
      ['状态', 'failed', 'status'],
    ]) {
      fireEvent.change(screen.getByRole('combobox', { name, exact: true }), { target: { value } })
      expect(setFilters).toHaveBeenLastCalledWith({ status: 'completed', [key]: value })
    }
    fireEvent.click(screen.getByRole('button', { name: '重置' }))
    expect(setFilters).toHaveBeenLastCalledWith({})
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

it('任务详情读取异步文本结果并提供复制，显示失败后允许重试', async () => {
  vi.mocked(fetchTaskResult)
    .mockRejectedValueOnce(new Error('结果暂时不可用'))
    .mockResolvedValueOnce({
      output: [{ content: [{ type: 'output_text', text: '原始文本结果' }] }],
    })
  const writeText = vi.fn().mockResolvedValue(undefined)
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } })
  render(
    <TaskDetail
      task={{ ...runningTask, task_kind: 'text', status: 'completed' }}
      onCancel={vi.fn()}
      onClose={vi.fn()}
    />
  )
  expect(await screen.findByRole('alert')).toHaveTextContent('结果暂时不可用')
  fireEvent.click(screen.getByRole('button', { name: '重试' }))
  expect(await screen.findByText('原始文本结果')).toBeInTheDocument()
  expect(fetchTaskResult).toHaveBeenLastCalledWith('task-running')
  fireEvent.click(screen.getByRole('button', { name: '复制' }))
  expect(writeText).toHaveBeenCalledWith('原始文本结果')
})

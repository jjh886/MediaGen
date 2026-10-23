import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cancelTask, fetchTasks, fetchTaskFilterOptions } from '../task-service'

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('task-service', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('按平台、分页参数请求服务端任务列表', async () => {
    const task = {
      id: 'task-1',
      platform: 'codex',
      task_kind: 'image',
      model_id: 'codex-image',
      prompt: 'test',
      status: 'pending',
      created_at: '2026-06-29T00:00:00Z',
    }
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: [task] }))

    const tasks = await fetchTasks('codex', 20, 10)

    expect(fetchMock).toHaveBeenCalledWith('/api/tasks?limit=20&offset=10&platform=codex', {
      headers: { Accept: 'application/json' },
    })
    expect(tasks).toEqual([task])
  })

  it('没有平台时只传分页参数', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: [] }))

    await fetchTasks('', 50, 0)

    expect(fetchMock).toHaveBeenCalledWith('/api/tasks?limit=50&offset=0', {
      headers: { Accept: 'application/json' },
    })
  })

  it('传递组合筛选并编码特殊字符', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: [] }))
    await fetchTasks('codex', 40, 40, {
      task_kind: 'video',
      account_id: 'a&b',
      status: 'failed',
      model_id: 'model+1',
    })
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/tasks?limit=40&offset=40&platform=codex&task_kind=video&account_id=a%26b&status=failed&model_id=model%2B1',
      {
        headers: { Accept: 'application/json' },
      }
    )
  })

  it('读取当前平台的历史筛选选项', async () => {
    const options = { account_ids: ['old-account'], model_ids: ['old-model'] }
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: options }))
    await expect(fetchTaskFilterOptions('codex')).resolves.toEqual(options)
    expect(fetchMock).toHaveBeenCalledWith('/api/tasks/filter-options?platform=codex', {
      headers: { Accept: 'application/json' },
    })
  })

  it('取消任务时调用服务端取消接口', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ ok: true }))

    await cancelTask('task-123')

    expect(fetchMock).toHaveBeenCalledWith('/api/tasks/task-123/cancel', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
    })
  })
})

it('队列服务传递独立类型筛选，文本结果按任务编号查询', async () => {
  const fetchMock = vi
    .fn()
    .mockImplementation(() => Promise.resolve(jsonResponse({ data: { items: [], total: 0 } })))
  vi.stubGlobal('fetch', fetchMock)
  try {
    const { fetchGenerationQueue } = await import('../generation-queue-service')
    const { fetchTaskResult } = await import('../task-service')
    await fetchGenerationQueue(20, 0, 'text')
    expect(fetchMock.mock.calls[0][0]).toBe('/api/generation-queue?limit=20&offset=0&taskKind=text')
    await fetchTaskResult('a/b')
    expect(fetchMock.mock.calls[1][0]).toBe('/api/tasks/a%2Fb/result')
  } finally {
    vi.unstubAllGlobals()
  }
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cancelTask, fetchTasks } from '../task-service'

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

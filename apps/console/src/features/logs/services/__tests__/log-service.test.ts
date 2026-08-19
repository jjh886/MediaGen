import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { clearLogs, fetchLogs } from '../log-service'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('log-service', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('按筛选条件请求服务端日志', async () => {
    const logs = [
      {
        id: 'log-1',
        timestamp: '2026-06-29T00:00:00Z',
        module: 'engine',
        level: 'info',
        summary: 'ok',
      },
    ]
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: logs }))

    const result = await fetchLogs({
      module: 'engine',
      level: 'info',
      keyword: 'ok',
      task_id: 'task-1',
      limit: 50,
      offset: 10,
    })

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/logs?module=engine&level=info&keyword=ok&task_id=task-1&limit=50&offset=10',
      { headers: { Accept: 'application/json' } }
    )
    expect(result).toEqual(logs)
  })

  it('没有筛选条件时传默认分页', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ data: [] }))

    await fetchLogs({})

    expect(fetchMock).toHaveBeenCalledWith('/api/logs?limit=40&offset=0', {
      headers: { Accept: 'application/json' },
    })
  })

  it('清理日志请求失败时不阻断界面操作', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ error: 'not implemented' }, 500))

    await expect(clearLogs()).resolves.toBeUndefined()
    expect(fetchMock).toHaveBeenCalledWith('/api/logs/clear', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
    })
  })
})

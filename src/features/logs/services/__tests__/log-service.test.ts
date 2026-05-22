import { describe, it, expect, vi, beforeEach } from 'vitest'
import { fetchLogs, clearLogs, type LogQuery } from '../log-service'

const mockInvoke = vi.fn()

vi.mock('@/shared/services/tauri-bridge', () => ({
  tauriInvoke: (...args: unknown[]) => mockInvoke(...args),
}))

beforeEach(() => {
  mockInvoke.mockReset()
})

describe('fetchLogs', () => {
  it('调用 get_logs 命令并返回结果', async () => {
    const logs = [
      {
        id: '1',
        timestamp: '2026-04-15 10:00:00',
        module: 'engine',
        level: 'info',
        summary: '测试日志',
      },
    ]
    mockInvoke.mockResolvedValue(logs)

    const result = await fetchLogs({})
    expect(mockInvoke).toHaveBeenCalledWith('get_logs', {
      module: null,
      level: null,
      keyword: null,
      limit: 200,
      offset: 0,
    })
    expect(result).toEqual(logs)
  })

  it('传递筛选参数', async () => {
    mockInvoke.mockResolvedValue([])
    const query: LogQuery = {
      module: 'engine',
      level: 'error',
      keyword: '失败',
      limit: 50,
      offset: 10,
    }

    await fetchLogs(query)
    expect(mockInvoke).toHaveBeenCalledWith('get_logs', {
      module: 'engine',
      level: 'error',
      keyword: '失败',
      limit: 50,
      offset: 10,
    })
  })

  it('空字符串 module/level/keyword 传 null', async () => {
    mockInvoke.mockResolvedValue([])

    await fetchLogs({ module: '', level: '', keyword: '' })
    expect(mockInvoke).toHaveBeenCalledWith('get_logs', {
      module: null,
      level: null,
      keyword: null,
      limit: 200,
      offset: 0,
    })
  })

  it('tauriInvoke 返回 undefined 时兜底为空数组', async () => {
    mockInvoke.mockResolvedValue(undefined)

    const result = await fetchLogs({})
    expect(result).toEqual([])
  })

  it('tauriInvoke 返回 null 时兜底为空数组', async () => {
    mockInvoke.mockResolvedValue(null)

    const result = await fetchLogs({})
    expect(result).toEqual([])
  })
})

describe('clearLogs', () => {
  it('调用 clear_logs 命令', async () => {
    mockInvoke.mockResolvedValue(undefined)

    await clearLogs()
    expect(mockInvoke).toHaveBeenCalledWith('clear_logs')
  })
})

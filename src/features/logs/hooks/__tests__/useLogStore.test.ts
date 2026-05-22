import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act } from '@testing-library/react'
import { useLogStore } from '../useLogStore'

const mockFetchLogs = vi.fn()

vi.mock('../../services/log-service', () => ({
  fetchLogs: (...args: unknown[]) => mockFetchLogs(...args),
}))

beforeEach(() => {
  mockFetchLogs.mockReset()
  useLogStore.setState({
    logs: [],
    loading: false,
    moduleFilter: '',
    levelFilter: '',
    keyword: '',
    expandedId: null,
  })
})

describe('useLogStore', () => {
  describe('初始状态', () => {
    it('默认值正确', () => {
      const state = useLogStore.getState()
      expect(state.logs).toEqual([])
      expect(state.loading).toBe(false)
      expect(state.moduleFilter).toBe('')
      expect(state.levelFilter).toBe('')
      expect(state.keyword).toBe('')
      expect(state.expandedId).toBeNull()
    })
  })

  describe('setModuleFilter', () => {
    it('设置 moduleFilter 并触发 load', async () => {
      mockFetchLogs.mockResolvedValue([])

      await act(async () => {
        useLogStore.getState().setModuleFilter('engine')
      })

      expect(useLogStore.getState().moduleFilter).toBe('engine')
      expect(mockFetchLogs).toHaveBeenCalled()
    })

    it('设置为空字符串时 query 不含 module', async () => {
      mockFetchLogs.mockResolvedValue([])

      await act(async () => {
        useLogStore.getState().setModuleFilter('')
      })

      expect(mockFetchLogs).toHaveBeenCalledWith({})
    })
  })

  describe('setLevelFilter', () => {
    it('设置 levelFilter 并触发 load', async () => {
      mockFetchLogs.mockResolvedValue([])

      await act(async () => {
        useLogStore.getState().setLevelFilter('error')
      })

      expect(useLogStore.getState().levelFilter).toBe('error')
      expect(mockFetchLogs).toHaveBeenCalled()
    })
  })

  describe('setKeyword', () => {
    it('设置 keyword 但不触发 load', () => {
      act(() => {
        useLogStore.getState().setKeyword('测试')
      })

      expect(useLogStore.getState().keyword).toBe('测试')
      expect(mockFetchLogs).not.toHaveBeenCalled()
    })
  })

  describe('toggleExpand', () => {
    it('展开日志条目', () => {
      act(() => {
        useLogStore.getState().toggleExpand('log-1')
      })
      expect(useLogStore.getState().expandedId).toBe('log-1')
    })

    it('再次点击同一条目收起', () => {
      useLogStore.setState({ expandedId: 'log-1' })

      act(() => {
        useLogStore.getState().toggleExpand('log-1')
      })
      expect(useLogStore.getState().expandedId).toBeNull()
    })

    it('点击不同条目切换到新条目', () => {
      useLogStore.setState({ expandedId: 'log-1' })

      act(() => {
        useLogStore.getState().toggleExpand('log-2')
      })
      expect(useLogStore.getState().expandedId).toBe('log-2')
    })
  })

  describe('load', () => {
    it('调用 fetchLogs 并更新 logs', async () => {
      const logs = [
        { id: '1', timestamp: '2026-04-15', module: 'engine', level: 'info', summary: 'test' },
      ]
      mockFetchLogs.mockResolvedValue(logs)

      await act(async () => {
        await useLogStore.getState().load()
      })

      expect(useLogStore.getState().logs).toEqual(logs)
      expect(useLogStore.getState().loading).toBe(false)
    })

    it('带筛选条件调用', async () => {
      mockFetchLogs.mockResolvedValue([])
      useLogStore.setState({
        moduleFilter: 'scheduler',
        levelFilter: 'warn',
        keyword: '超时',
      })

      await act(async () => {
        await useLogStore.getState().load()
      })

      expect(mockFetchLogs).toHaveBeenCalledWith({
        module: 'scheduler',
        level: 'warn',
        keyword: '超时',
      })
    })

    it('空筛选条件不加入 query', async () => {
      mockFetchLogs.mockResolvedValue([])

      await act(async () => {
        await useLogStore.getState().load()
      })

      expect(mockFetchLogs).toHaveBeenCalledWith({})
    })

    it('loading 状态在请求期间为 true', async () => {
      let resolvePromise: (v: unknown[]) => void
      mockFetchLogs.mockReturnValue(
        new Promise((resolve) => {
          resolvePromise = resolve
        })
      )

      let loadPromise: Promise<void>
      act(() => {
        loadPromise = useLogStore.getState().load()
      })

      expect(useLogStore.getState().loading).toBe(true)

      await act(async () => {
        resolvePromise!([])
        await loadPromise!
      })

      expect(useLogStore.getState().loading).toBe(false)
    })
  })
})

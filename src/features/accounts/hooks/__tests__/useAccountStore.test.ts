import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act } from '@testing-library/react'
import { useAccountStore } from '../useAccountStore'

const mockFetchAccounts = vi.fn()
const mockDeleteAccount = vi.fn()
const mockToggleAccount = vi.fn()
const mockUpdateAccount = vi.fn()
const mockCheckAccountsLogin = vi.fn()

vi.mock('../../services/account-service', () => ({
  fetchAccounts: (...args: unknown[]) => mockFetchAccounts(...args),
  deleteAccount: (...args: unknown[]) => mockDeleteAccount(...args),
  toggleAccount: (...args: unknown[]) => mockToggleAccount(...args),
  updateAccount: (...args: unknown[]) => mockUpdateAccount(...args),
  checkAccountsLogin: (...args: unknown[]) => mockCheckAccountsLogin(...args),
}))

beforeEach(() => {
  mockFetchAccounts.mockReset()
  mockDeleteAccount.mockReset()
  mockToggleAccount.mockReset()
  mockUpdateAccount.mockReset()
  mockCheckAccountsLogin.mockReset()

  // 重置 store 状态
  useAccountStore.setState({
    accounts: [],
    loading: false,
    checking: false,
    activePlatform: '',
  })
})

describe('useAccountStore', () => {
  describe('初始状态', () => {
    it('默认值正确', () => {
      const state = useAccountStore.getState()
      expect(state.accounts).toEqual([])
      expect(state.loading).toBe(false)
      expect(state.checking).toBe(false)
      expect(state.activePlatform).toBe('')
    })
  })

  describe('fetch', () => {
    it('调用 fetchAccounts 并更新 accounts', async () => {
      const accounts = [{ id: 'a1', platform: 'flow' }]
      mockFetchAccounts.mockResolvedValue(accounts)

      await act(async () => {
        await useAccountStore.getState().fetch()
      })

      const state = useAccountStore.getState()
      expect(state.accounts).toEqual(accounts)
      expect(state.loading).toBe(false)
    })

    it('使用传入的 platform 参数', async () => {
      mockFetchAccounts.mockResolvedValue([])

      await act(async () => {
        await useAccountStore.getState().fetch('doubao')
      })

      expect(mockFetchAccounts).toHaveBeenCalledWith('doubao')
    })

    it('未传 platform 时使用 activePlatform', async () => {
      mockFetchAccounts.mockResolvedValue([])
      useAccountStore.setState({ activePlatform: 'flow' })

      await act(async () => {
        await useAccountStore.getState().fetch()
      })

      expect(mockFetchAccounts).toHaveBeenCalledWith('flow')
    })

    it('loading 状态在请求期间为 true', async () => {
      let resolvePromise: (v: unknown[]) => void
      mockFetchAccounts.mockReturnValue(
        new Promise((resolve) => {
          resolvePromise = resolve
        })
      )

      let fetchPromise: Promise<void>
      act(() => {
        fetchPromise = useAccountStore.getState().fetch()
      })

      expect(useAccountStore.getState().loading).toBe(true)

      await act(async () => {
        resolvePromise!([])
        await fetchPromise!
      })

      expect(useAccountStore.getState().loading).toBe(false)
    })

    it('fetchAccounts 抛错后 loading 仍恢复为 false', async () => {
      mockFetchAccounts.mockRejectedValue(new Error('fail'))

      await act(async () => {
        try {
          await useAccountStore.getState().fetch()
        } catch {
          // 预期抛错
        }
      })

      expect(useAccountStore.getState().loading).toBe(false)
    })
  })

  describe('setActivePlatform', () => {
    it('设置平台并触发 fetch', async () => {
      mockFetchAccounts.mockResolvedValue([])

      await act(async () => {
        useAccountStore.getState().setActivePlatform('doubao')
      })

      expect(useAccountStore.getState().activePlatform).toBe('doubao')
      expect(mockFetchAccounts).toHaveBeenCalledWith('doubao')
    })
  })

  describe('remove', () => {
    it('调用 deleteAccount 后刷新列表', async () => {
      mockDeleteAccount.mockResolvedValue(undefined)
      mockFetchAccounts.mockResolvedValue([])

      await act(async () => {
        await useAccountStore.getState().remove('acc-001')
      })

      expect(mockDeleteAccount).toHaveBeenCalledWith('acc-001')
      expect(mockFetchAccounts).toHaveBeenCalled()
    })
  })

  describe('toggle', () => {
    it('调用 toggleAccount 后刷新列表', async () => {
      mockToggleAccount.mockResolvedValue(undefined)
      mockFetchAccounts.mockResolvedValue([])

      await act(async () => {
        await useAccountStore.getState().toggle('acc-002', true)
      })

      expect(mockToggleAccount).toHaveBeenCalledWith('acc-002', true)
      expect(mockFetchAccounts).toHaveBeenCalled()
    })
  })

  describe('update', () => {
    it('调用 updateAccount 后刷新列表', async () => {
      mockUpdateAccount.mockResolvedValue(undefined)
      mockFetchAccounts.mockResolvedValue([])

      await act(async () => {
        await useAccountStore.getState().update('acc-001', { tier: 'pro' })
      })

      expect(mockUpdateAccount).toHaveBeenCalledWith('acc-001', { tier: 'pro' })
      expect(mockFetchAccounts).toHaveBeenCalled()
    })
  })

  describe('checkLogin', () => {
    it('调用 checkAccountsLogin 并返回结果', async () => {
      mockCheckAccountsLogin.mockResolvedValue({ checked: 10, expired: 3 })
      mockFetchAccounts.mockResolvedValue([])

      let result: { checked: number; expired: number }
      await act(async () => {
        result = await useAccountStore.getState().checkLogin()
      })

      expect(result!).toEqual({ checked: 10, expired: 3 })
      expect(useAccountStore.getState().checking).toBe(false)
    })

    it('checking 状态在请求期间为 true', async () => {
      let resolveCheck: (v: { checked: number; expired: number }) => void
      mockCheckAccountsLogin.mockReturnValue(
        new Promise((resolve) => {
          resolveCheck = resolve
        })
      )
      mockFetchAccounts.mockResolvedValue([])

      let checkPromise: Promise<{ checked: number; expired: number }>
      act(() => {
        checkPromise = useAccountStore.getState().checkLogin()
      })

      expect(useAccountStore.getState().checking).toBe(true)

      await act(async () => {
        resolveCheck!({ checked: 0, expired: 0 })
        await checkPromise!
      })

      expect(useAccountStore.getState().checking).toBe(false)
    })

    it('checkAccountsLogin 抛错后 checking 恢复为 false', async () => {
      mockCheckAccountsLogin.mockRejectedValue(new Error('fail'))
      mockFetchAccounts.mockResolvedValue([])

      await act(async () => {
        try {
          await useAccountStore.getState().checkLogin()
        } catch {
          // 预期抛错
        }
      })

      expect(useAccountStore.getState().checking).toBe(false)
    })

    it('检查完后刷新账号列表', async () => {
      mockCheckAccountsLogin.mockResolvedValue({ checked: 5, expired: 1 })
      mockFetchAccounts.mockResolvedValue([])

      await act(async () => {
        await useAccountStore.getState().checkLogin()
      })

      expect(mockFetchAccounts).toHaveBeenCalled()
    })
  })
})

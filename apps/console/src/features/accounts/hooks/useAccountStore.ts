import { create } from 'zustand'
import type { Account } from '../types/account'
import {
  fetchAccounts,
  deleteAccount,
  toggleAccount,
  updateAccount,
  checkAccountsLogin,
  type AccountConfigUpdate,
} from '../services/account-service'
import {
  appendUnique,
  listErrorMessage,
  LIST_PAGE_SIZE,
  runPageRequest,
} from '@/shared/services/list-pagination'

interface AccountStore {
  accounts: Account[]
  loading: boolean
  loadingMore: boolean
  hasMore: boolean
  nextOffset: number
  error: string
  checking: boolean
  activePlatform: string
  fetch: (platform?: string) => Promise<void>
  loadMore: () => Promise<void>
  syncLatest: () => Promise<void>
  setActivePlatform: (platform: string) => void
  remove: (id: string) => Promise<void>
  toggle: (id: string, disabled: boolean) => Promise<void>
  update: (id: string, updates: AccountConfigUpdate) => Promise<void>
  checkLogin: () => Promise<{ checked: number; expired: number; limited: number }>
}

export const useAccountStore = create<AccountStore>((set, get) => ({
  accounts: [],
  loading: false,
  loadingMore: false,
  hasMore: true,
  nextOffset: 0,
  error: '',
  checking: false,
  activePlatform: '',

  fetch: async (platform?: string) => {
    const p = platform ?? get().activePlatform
    set({ error: '' })
    const accounts = await runPageRequest(
      (loading) => set({ loading }),
      () => fetchAccounts(p, LIST_PAGE_SIZE, 0),
      (error) => set({ error: listErrorMessage(error, '账号加载失败。') })
    )
    set({
      accounts,
      hasMore: accounts.length === LIST_PAGE_SIZE,
      nextOffset: accounts.length,
    })
  },

  loadMore: async () => {
    const { activePlatform, hasMore, loadingMore, nextOffset } = get()
    if (!hasMore || loadingMore) return
    set({ error: '' })
    const page = await runPageRequest(
      (loadingMore) => set({ loadingMore }),
      () => fetchAccounts(activePlatform, LIST_PAGE_SIZE, nextOffset),
      (error) => set({ error: listErrorMessage(error, '账号加载失败。') })
    )
    set((state) => ({
      accounts: appendUnique(state.accounts, page, (account) => account.id),
      hasMore: page.length === LIST_PAGE_SIZE,
      nextOffset: state.nextOffset + page.length,
    }))
  },

  syncLatest: async () => {
    const state = get()
    // 向下浏览后不自动改变列表长度和位置，只同步常用的首屏账号。
    if (!state.activePlatform || state.accounts.length > LIST_PAGE_SIZE) return
    if (state.loading || state.loadingMore || state.checking) return
    try {
      const accounts = await fetchAccounts(state.activePlatform, LIST_PAGE_SIZE, 0)
      if (get().activePlatform !== state.activePlatform) return
      set({
        accounts,
        hasMore: accounts.length === LIST_PAGE_SIZE,
        nextOffset: accounts.length,
      })
    } catch {
      // 轻量同步失败时保留当前列表，下一轮继续尝试。
    }
  },

  setActivePlatform: (platform) => {
    set({ activePlatform: platform })
    void get()
      .fetch(platform)
      .catch(() => undefined)
  },

  remove: async (id) => {
    await deleteAccount(id)
    set((state) => ({
      accounts: state.accounts.filter((account) => account.id !== id),
      nextOffset: Math.max(0, state.nextOffset - 1),
    }))
  },

  toggle: async (id, disabled) => {
    const updated = await toggleAccount(id, disabled)
    set((state) => ({
      accounts: state.accounts.map((account) => (account.id === id ? updated : account)),
    }))
  },

  update: async (id, updates) => {
    const updated = await updateAccount(id, updates)
    set((state) => ({
      accounts: state.accounts.map((account) => (account.id === id ? updated : account)),
    }))
  },

  checkLogin: async () => {
    set({ checking: true })
    try {
      const result = await checkAccountsLogin()
      // 检查完后刷新列表以获取最新状态
      await get().fetch()
      return result
    } finally {
      set({ checking: false })
    }
  },
}))

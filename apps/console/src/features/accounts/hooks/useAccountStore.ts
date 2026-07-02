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

interface AccountStore {
  accounts: Account[]
  loading: boolean
  checking: boolean
  activePlatform: string
  fetch: (platform?: string) => Promise<void>
  setActivePlatform: (platform: string) => void
  remove: (id: string) => Promise<void>
  toggle: (id: string, disabled: boolean) => Promise<void>
  update: (id: string, updates: AccountConfigUpdate) => Promise<void>
  checkLogin: () => Promise<{ checked: number; expired: number }>
}

export const useAccountStore = create<AccountStore>((set, get) => ({
  accounts: [],
  loading: false,
  checking: false,
  activePlatform: '',

  fetch: async (platform?: string) => {
    const p = platform ?? get().activePlatform
    set({ loading: true })
    try {
      const accounts = await fetchAccounts(p)
      set({ accounts })
    } finally {
      set({ loading: false })
    }
  },

  setActivePlatform: (platform) => {
    set({ activePlatform: platform })
    get().fetch(platform)
  },

  remove: async (id) => {
    await deleteAccount(id)
    await get().fetch()
  },

  toggle: async (id, disabled) => {
    await toggleAccount(id, disabled)
    await get().fetch()
  },

  update: async (id, updates) => {
    await updateAccount(id, updates)
    await get().fetch()
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

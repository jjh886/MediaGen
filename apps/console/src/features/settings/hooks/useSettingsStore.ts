import { create } from 'zustand'
import {
  getAllSettings,
  setSetting,
  checkBrowserRuntimeVersion,
  getCacheSize,
  clearCache,
  type AppSettings,
} from '@/shared/services/settings-service'

interface SettingsState {
  settings: AppSettings | null
  loading: boolean
  saving: boolean
  runtimeVersion: string
  cacheSize: string

  load: () => Promise<void>
  update: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void
  save: () => Promise<void>
  detectBrowserRuntime: () => Promise<void>
  refreshCacheSize: () => Promise<void>
  cleanCache: () => Promise<void>
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  loading: false,
  saving: false,
  runtimeVersion: '',
  cacheSize: '',

  load: async () => {
    set({ loading: true })
    const [settings, cacheSize] = await Promise.all([getAllSettings(), getCacheSize()])
    set({ settings, cacheSize, loading: false })
  },

  update: (key, value) => {
    const prev = get().settings
    if (!prev) return
    set({ settings: { ...prev, [key]: value } })
  },

  save: async () => {
    const { settings } = get()
    if (!settings) return
    set({ saving: true })
    const keys = Object.keys(settings) as (keyof AppSettings)[]
    await Promise.all(keys.map((k) => setSetting(k, settings[k])))
    set({ saving: false })
  },

  detectBrowserRuntime: async () => {
    const version = await checkBrowserRuntimeVersion()
    set({ runtimeVersion: version })
  },

  refreshCacheSize: async () => {
    const size = await getCacheSize()
    set({ cacheSize: size })
  },

  cleanCache: async () => {
    await clearCache()
    const size = await getCacheSize()
    set({ cacheSize: size })
  },
}))

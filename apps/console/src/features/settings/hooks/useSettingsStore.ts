import { create } from 'zustand'
import {
  getAllSettings,
  setSetting,
  getCacheSize,
  clearCache,
  type AppSettings,
} from '@/shared/services/settings-service'

interface SettingsState {
  settings: AppSettings | null
  savedSettings: AppSettings | null
  loading: boolean
  saving: boolean
  loadError: string
  saveError: string
  saveMessage: string
  cacheSize: string

  load: () => Promise<void>
  update: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void
  save: () => Promise<void>
  refreshCacheSize: () => Promise<void>
  cleanCache: () => Promise<void>
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  savedSettings: null,
  loading: false,
  saving: false,
  loadError: '',
  saveError: '',
  saveMessage: '',
  cacheSize: '',

  load: async () => {
    set({ loading: true, loadError: '' })
    try {
      const [settings, cacheSize] = await Promise.all([getAllSettings(), getCacheSize()])
      set({ settings, savedSettings: settings, cacheSize, loading: false })
    } catch (error) {
      set({ loadError: errorMessage(error), loading: false })
    }
  },

  update: (key, value) => {
    const prev = get().settings
    if (!prev) return
    set({ settings: { ...prev, [key]: value }, saveError: '', saveMessage: '' })
  },

  save: async () => {
    const { settings, savedSettings } = get()
    if (!settings) return
    const keys = savedSettings
      ? changedKeys(settings, savedSettings)
      : (Object.keys(settings) as (keyof AppSettings)[])
    if (keys.length === 0) {
      set({ saveMessage: '没有需要保存的改动。' })
      return
    }
    set({ saving: true, saveError: '', saveMessage: '' })
    try {
      await Promise.all(keys.map((key) => setSetting(key, settings[key])))
      set({ savedSettings: settings, saveMessage: saveMessage(keys), saving: false })
    } catch (error) {
      set({ saveError: errorMessage(error), saving: false })
    }
  },

  refreshCacheSize: async () => {
    const size = await getCacheSize()
    set({ cacheSize: size })
  },

  cleanCache: async () => {
    try {
      await clearCache()
      const size = await getCacheSize()
      set({ cacheSize: size })
    } catch (error) {
      set({ saveError: errorMessage(error) })
    }
  },
}))

function changedKeys(settings: AppSettings, saved: AppSettings): (keyof AppSettings)[] {
  return (Object.keys(settings) as (keyof AppSettings)[]).filter(
    (key) => settings[key] !== saved[key]
  )
}

function saveMessage(keys: (keyof AppSettings)[]) {
  const restartKeys: (keyof AppSettings)[] = ['api_port', 'api_end_port', 'api_address']
  const restartRequired = keys.some((key) => restartKeys.includes(key))
  return restartRequired ? '设置已保存，端口或监听地址变更需要重启后生效。' : '设置已保存。'
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : '操作失败，请稍后重试。'
}

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act } from '@testing-library/react'
import { useSettingsStore } from '../useSettingsStore'
import type { AppSettings } from '@/shared/services/settings-service'

const mockGetAllSettings = vi.fn()
const mockSetSetting = vi.fn()
const mockCheckChromeVersion = vi.fn()
const mockGetCacheSize = vi.fn()
const mockClearCache = vi.fn()

vi.mock('@/shared/services/settings-service', () => ({
  getAllSettings: (...args: unknown[]) => mockGetAllSettings(...args),
  setSetting: (...args: unknown[]) => mockSetSetting(...args),
  checkChromeVersion: (...args: unknown[]) => mockCheckChromeVersion(...args),
  getCacheSize: (...args: unknown[]) => mockGetCacheSize(...args),
  clearCache: (...args: unknown[]) => mockClearCache(...args),
}))

// 测试用 fixture，字段值故意与源码 DEFAULTS 不同以避免重复代码检测
const defaultSettings: AppSettings = {
  api_port: 7777,
  api_end_port: 7786,
  api_ports: '7777-7780',
  api_port_count: 10,
  api_address: '192.168.1.1',
  api_allowed_origins: '*',
  image_task_concurrency: 64,
  image_task_queue_limit: 512,
  flow_mirror_url: 'https://mirror.example.com',
  chrome_path: '/opt/chrome/chrome',
  check_interval_minutes: 15,
  cooldown_default_seconds: 120,
  cache_dir: '/tmp/test-cache',
  auto_register_enabled: false,
  auto_register_password_mode: 'default',
  auto_register_password: 'TestPass@2026',
  auto_register_birthday_mode: 'default',
  auto_register_birthday: '2000-01-01',
  platform_tier_overrides: '{"doubao_free":{"daily_limit":10}}',
}

beforeEach(() => {
  mockGetAllSettings.mockReset()
  mockSetSetting.mockReset()
  mockCheckChromeVersion.mockReset()
  mockGetCacheSize.mockReset()
  mockClearCache.mockReset()

  useSettingsStore.setState({
    settings: null,
    loading: false,
    saving: false,
    chromeVersion: '',
    cacheSize: '',
  })
})

describe('useSettingsStore', () => {
  describe('初始状态', () => {
    it('默认值正确', () => {
      const state = useSettingsStore.getState()
      expect(state.settings).toBeNull()
      expect(state.loading).toBe(false)
      expect(state.saving).toBe(false)
      expect(state.chromeVersion).toBe('')
      expect(state.cacheSize).toBe('')
    })
  })

  describe('load', () => {
    it('并行加载 settings 和 cacheSize', async () => {
      mockGetAllSettings.mockResolvedValue(defaultSettings)
      mockGetCacheSize.mockResolvedValue('128 MB')

      await act(async () => {
        await useSettingsStore.getState().load()
      })

      const state = useSettingsStore.getState()
      expect(state.settings).toEqual(defaultSettings)
      expect(state.cacheSize).toBe('128 MB')
      expect(state.loading).toBe(false)
    })

    it('loading 状态在请求期间为 true', async () => {
      let resolveSettings: (v: AppSettings) => void
      let resolveCacheSize: (v: string) => void
      mockGetAllSettings.mockReturnValue(
        new Promise((resolve) => {
          resolveSettings = resolve
        })
      )
      mockGetCacheSize.mockReturnValue(
        new Promise((resolve) => {
          resolveCacheSize = resolve
        })
      )

      let loadPromise: Promise<void>
      act(() => {
        loadPromise = useSettingsStore.getState().load()
      })

      expect(useSettingsStore.getState().loading).toBe(true)

      await act(async () => {
        resolveSettings!(defaultSettings)
        resolveCacheSize!('0 MB')
        await loadPromise!
      })

      expect(useSettingsStore.getState().loading).toBe(false)
    })
  })

  describe('update', () => {
    it('更新单个字段', () => {
      useSettingsStore.setState({ settings: { ...defaultSettings } })

      act(() => {
        useSettingsStore.getState().update('api_port', 8080)
      })

      expect(useSettingsStore.getState().settings!.api_port).toBe(8080)
    })

    it('settings 为 null 时不操作', () => {
      act(() => {
        useSettingsStore.getState().update('api_port', 8080)
      })

      expect(useSettingsStore.getState().settings).toBeNull()
    })

    it('不影响其他字段', () => {
      useSettingsStore.setState({ settings: { ...defaultSettings } })

      act(() => {
        useSettingsStore.getState().update('chrome_path', '/new/path')
      })

      const s = useSettingsStore.getState().settings!
      expect(s.chrome_path).toBe('/new/path')
      expect(s.api_port).toBe(7777)
    })
  })

  describe('save', () => {
    it('逐个调用 setSetting 保存所有字段', async () => {
      useSettingsStore.setState({ settings: { ...defaultSettings } })
      mockSetSetting.mockResolvedValue(undefined)

      await act(async () => {
        await useSettingsStore.getState().save()
      })

      const keys = Object.keys(defaultSettings)
      expect(mockSetSetting).toHaveBeenCalledTimes(keys.length)
      expect(useSettingsStore.getState().saving).toBe(false)
    })

    it('settings 为 null 时不调用 setSetting', async () => {
      await act(async () => {
        await useSettingsStore.getState().save()
      })

      expect(mockSetSetting).not.toHaveBeenCalled()
    })

    it('saving 状态在保存期间为 true', async () => {
      useSettingsStore.setState({ settings: { ...defaultSettings } })
      let resolveAll: (v: undefined) => void
      mockSetSetting.mockReturnValue(
        new Promise((resolve) => {
          resolveAll = resolve
        })
      )

      let savePromise: Promise<void>
      act(() => {
        savePromise = useSettingsStore.getState().save()
      })

      expect(useSettingsStore.getState().saving).toBe(true)

      await act(async () => {
        resolveAll!(undefined)
        await savePromise!
      })

      expect(useSettingsStore.getState().saving).toBe(false)
    })
  })

  describe('detectChrome', () => {
    it('检测 Chrome 版本', async () => {
      useSettingsStore.setState({ settings: { ...defaultSettings } })
      mockCheckChromeVersion.mockResolvedValue('Chrome 124.0')

      await act(async () => {
        await useSettingsStore.getState().detectChrome()
      })

      expect(mockCheckChromeVersion).toHaveBeenCalledWith('/opt/chrome/chrome')
      expect(useSettingsStore.getState().chromeVersion).toBe('Chrome 124.0')
    })

    it('settings 为 null 时使用空字符串路径', async () => {
      mockCheckChromeVersion.mockResolvedValue('未找到')

      await act(async () => {
        await useSettingsStore.getState().detectChrome()
      })

      expect(mockCheckChromeVersion).toHaveBeenCalledWith('')
    })
  })

  describe('refreshCacheSize', () => {
    it('刷新缓存大小', async () => {
      mockGetCacheSize.mockResolvedValue('512 MB')

      await act(async () => {
        await useSettingsStore.getState().refreshCacheSize()
      })

      expect(useSettingsStore.getState().cacheSize).toBe('512 MB')
    })
  })

  describe('cleanCache', () => {
    it('清除缓存后刷新大小', async () => {
      mockClearCache.mockResolvedValue(undefined)
      mockGetCacheSize.mockResolvedValue('0 MB')

      await act(async () => {
        await useSettingsStore.getState().cleanCache()
      })

      expect(mockClearCache).toHaveBeenCalled()
      expect(mockGetCacheSize).toHaveBeenCalled()
      expect(useSettingsStore.getState().cacheSize).toBe('0 MB')
    })
  })
})

import { tauriInvoke } from '@/shared/services/tauri-bridge'

export interface AppSettings {
  api_port: number
  api_end_port: number
  api_ports: string
  api_port_count: number
  api_address: string
  api_allowed_origins: string
  image_task_concurrency: number
  image_task_queue_limit: number
  flow_mirror_url: string
  chrome_path: string
  check_interval_minutes: number
  cooldown_default_seconds: number
  cache_dir: string
  // 平台默认配额覆盖（JSON 格式存储，key=platform_tier，如 "doubao_free"）
  platform_tier_overrides: string
}

export interface TierOverride {
  concurrency_limit?: number
  daily_limit?: number
  total_limit?: number
  cooldown_seconds?: number
  task_interval_seconds?: number
}

const DEFAULTS: AppSettings = {
  api_port: 18080,
  api_end_port: 18089,
  api_ports: '',
  api_port_count: 10,
  api_address: '127.0.0.1',
  api_allowed_origins: '*',
  image_task_concurrency: 64,
  image_task_queue_limit: 512,
  flow_mirror_url: '',
  chrome_path: '',
  check_interval_minutes: 30,
  cooldown_default_seconds: 300,
  cache_dir: '',
  platform_tier_overrides: '{}',
}

const isTauri = '__TAURI_INTERNALS__' in window
const LS_PREFIX = 'settings:'

function deserializeValue<K extends keyof AppSettings>(
  key: K,
  raw: string | null | undefined
): AppSettings[K] {
  if (raw === null || raw === undefined) return DEFAULTS[key]
  if (typeof DEFAULTS[key] === 'string') {
    try {
      const parsed = JSON.parse(raw)
      // 字符串配置里可能存的是 JSON 原文，例如平台配额覆盖；这类值不能被误转成对象。
      return (typeof parsed === 'string' ? parsed : raw) as AppSettings[K]
    } catch {
      return raw as AppSettings[K]
    }
  }
  try {
    return JSON.parse(raw) as AppSettings[K]
  } catch {
    return DEFAULTS[key]
  }
}

export async function getSetting<K extends keyof AppSettings>(key: K): Promise<AppSettings[K]> {
  if (!isTauri) {
    const raw = localStorage.getItem(LS_PREFIX + key)
    return deserializeValue(key, raw)
  }
  try {
    const raw = await tauriInvoke<string | null>('get_settings', { key: String(key) })
    return deserializeValue(key, raw)
  } catch {
    /* command may not exist yet */
  }
  return DEFAULTS[key]
}

function serializeValue(value: unknown): string {
  if (typeof value === 'string') return value
  return JSON.stringify(value)
}

export async function setSetting<K extends keyof AppSettings>(
  key: K,
  value: AppSettings[K]
): Promise<void> {
  const serialized = serializeValue(value)
  if (!isTauri) {
    localStorage.setItem(LS_PREFIX + key, serialized)
    return
  }
  try {
    await tauriInvoke('set_setting', { key: String(key), value: serialized })
  } catch {
    console.warn(`[settings-service] set_setting(${key}) 调用失败`)
    localStorage.setItem(LS_PREFIX + key, serialized)
  }
}

export async function getAllSettings(): Promise<AppSettings> {
  const keys = Object.keys(DEFAULTS) as (keyof AppSettings)[]
  const entries = await Promise.all(keys.map(async (k) => [k, await getSetting(k)] as const))
  return Object.fromEntries(entries) as unknown as AppSettings
}

export async function checkChromeAvailable(): Promise<{ ok: boolean; message: string }> {
  if (!isTauri) return { ok: true, message: 'Chrome (mock)' }
  try {
    const version = await tauriInvoke<string>('check_chrome_available')
    return { ok: true, message: version }
  } catch (e) {
    return { ok: false, message: String(e) }
  }
}

export async function checkChromeVersion(path: string): Promise<string> {
  if (!isTauri) return 'Chrome 124.0.6367.91 (mock)'
  try {
    return await tauriInvoke<string>('check_chrome_version', { path })
  } catch {
    return '检测失败'
  }
}

export async function getCacheSize(): Promise<string> {
  if (!isTauri) return '256 MB (mock)'
  try {
    return await tauriInvoke<string>('get_cache_size')
  } catch {
    console.warn('[settings-service] get_cache_size 调用失败')
    return '未知'
  }
}

export async function clearCache(): Promise<void> {
  if (!isTauri) return
  await tauriInvoke('clear_cache')
}

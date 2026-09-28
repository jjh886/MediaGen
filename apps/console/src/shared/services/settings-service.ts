import { apiGet, apiPost } from '@/shared/services/http-api-client'

export interface AppSettings {
  api_port: number
  api_address: string
  api_allowed_origins: string
  image_task_concurrency: number
  image_task_queue_limit: number
  video_task_concurrency: number
  video_task_queue_limit: number
  check_interval_minutes: number
  cooldown_default_seconds: number
  cache_dir: string
  // 平台默认配额覆盖（JSON 格式存储，key=platform_tier，如 "doubao_free"）
  platform_tier_overrides: string
  // 平台生成参数（JSON 格式存储，key=platform_id，字段由插件解释）
  platform_generation_configs: string
}

export interface TierOverride {
  concurrency_limit?: number
  daily_limit?: number
  total_limit?: number
  cooldown_seconds?: number
  task_interval_seconds?: number
}

// 导出统一默认值，测试与界面都从同一事实来源构造设置，避免字段新增后两边漂移。
export const DEFAULT_SETTINGS: AppSettings = {
  api_port: 18080,
  api_address: '0.0.0.0',
  api_allowed_origins: '*',
  image_task_concurrency: 64,
  image_task_queue_limit: 512,
  video_task_concurrency: 8,
  video_task_queue_limit: 64,
  check_interval_minutes: 30,
  cooldown_default_seconds: 300,
  cache_dir: '',
  platform_tier_overrides: '{}',
  platform_generation_configs: '{}',
}

function deserializeValue<K extends keyof AppSettings>(key: K, raw: unknown): AppSettings[K] {
  if (raw === null || raw === undefined) return DEFAULT_SETTINGS[key]
  if (typeof DEFAULT_SETTINGS[key] === 'string') {
    if (typeof raw === 'string') return raw as AppSettings[K]
    return JSON.stringify(raw) as AppSettings[K]
  }
  if (typeof raw === 'number') {
    return raw as AppSettings[K]
  }
  try {
    return JSON.parse(String(raw)) as AppSettings[K]
  } catch {
    return DEFAULT_SETTINGS[key]
  }
}

export async function getSetting<K extends keyof AppSettings>(key: K): Promise<AppSettings[K]> {
  const settings = await getServerSettings()
  return deserializeValue(key, settings[key])
}

export async function setSetting<K extends keyof AppSettings>(
  key: K,
  value: AppSettings[K]
): Promise<void> {
  // Web Console 只是服务端的控制面板，设置必须落到服务端，不能留在浏览器本地。
  await apiPost('/api/settings', { [key]: value } as Partial<Record<keyof AppSettings, unknown>>)
}

export async function getAllSettings(): Promise<AppSettings> {
  const keys = Object.keys(DEFAULT_SETTINGS) as (keyof AppSettings)[]
  const serverSettings = await getServerSettings()
  const entries = keys.map((k) => [k, deserializeValue(k, serverSettings[k])] as const)
  return Object.fromEntries(entries) as unknown as AppSettings
}

export async function getCacheSize(): Promise<string> {
  return '暂未接入真实清理'
}

export async function clearCache(): Promise<void> {
  await apiPost('/api/cache/clear')
}

async function getServerSettings(): Promise<Partial<Record<keyof AppSettings, unknown>>> {
  return await apiGet<Partial<Record<keyof AppSettings, unknown>>>('/api/settings')
}

import { apiGet } from './http-api-client'

export interface PluginInfo {
  id: string
  platform_id: string
  display_name: string
  model_count: number
  supports_registration: boolean
}

export interface PluginTierQuota {
  daily_limit: number
  total_limit: number
  concurrency_limit: number
  task_interval_seconds: number
}

export interface PluginQuotaInfo {
  platform_id: string
  display_name: string
  tiers: Record<string, PluginTierQuota>
}

export async function fetchPlugins(): Promise<PluginInfo[]> {
  const plugins = await apiGet<(PluginInfo & { models?: unknown[] })[]>('/api/plugins')
  return plugins.map((plugin) => ({
    id: plugin.platform_id,
    platform_id: plugin.platform_id,
    display_name: plugin.display_name,
    model_count: plugin.model_count ?? plugin.models?.length ?? 0,
    supports_registration: plugin.supports_registration ?? false,
  }))
}

export async function fetchPluginQuotas(): Promise<PluginQuotaInfo[]> {
  return await apiGet<PluginQuotaInfo[]>('/api/plugins')
}

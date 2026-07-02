import { apiGet } from './http-api-client'

export interface PluginInfo {
  platform_id: string
  display_name: string
  model_count: number
  supports_registration: boolean
}

export async function fetchPlugins(): Promise<PluginInfo[]> {
  const plugins = await apiGet<(PluginInfo & { models?: unknown[] })[]>('/api/plugins')
  return plugins.map((plugin) => ({
    platform_id: plugin.platform_id,
    display_name: plugin.display_name,
    model_count: plugin.model_count ?? plugin.models?.length ?? 0,
    supports_registration: plugin.supports_registration ?? false,
  }))
}

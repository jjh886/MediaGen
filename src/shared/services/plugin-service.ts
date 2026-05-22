import { tauriInvoke } from './tauri-bridge'

export interface PluginInfo {
  platform_id: string
  display_name: string
  model_count: number
  supports_registration: boolean
}

const isTauri = '__TAURI_INTERNALS__' in window

export async function fetchPlugins(): Promise<PluginInfo[]> {
  if (!isTauri) {
    return [
      { platform_id: 'flow', display_name: 'Flow', model_count: 1, supports_registration: false },
      {
        platform_id: 'doubao',
        display_name: '豆包',
        model_count: 2,
        supports_registration: false,
      },
    ]
  }
  try {
    return (await tauriInvoke<PluginInfo[]>('get_plugins')) ?? []
  } catch {
    console.warn('[plugin-service] get_plugins 调用失败')
    return []
  }
}

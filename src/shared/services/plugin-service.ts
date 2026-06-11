import { tauriInvoke } from './tauri-bridge'

export interface PluginInfo {
  platform_id: string
  display_name: string
  model_count: number
  supports_registration: boolean
}

const isTauri = '__TAURI_INTERNALS__' in window

// 浏览器预览没有 Tauri 后端时，用与 manifest sort_order 一致的兜底顺序展示插件。
const fallbackPlugins: PluginInfo[] = [
  { platform_id: 'chatgpt', display_name: 'ChatGPT', model_count: 1, supports_registration: false },
  { platform_id: 'codex', display_name: 'codex', model_count: 1, supports_registration: false },
  { platform_id: 'jimeng', display_name: '即梦', model_count: 5, supports_registration: false },
  { platform_id: 'doubao', display_name: '豆包', model_count: 2, supports_registration: false },
  { platform_id: 'flow', display_name: 'Flow', model_count: 3, supports_registration: false },
]

export async function fetchPlugins(): Promise<PluginInfo[]> {
  if (!isTauri) {
    return fallbackPlugins
  }
  try {
    return (await tauriInvoke<PluginInfo[]>('get_plugins')) ?? []
  } catch {
    console.warn('[plugin-service] get_plugins 调用失败')
    return []
  }
}

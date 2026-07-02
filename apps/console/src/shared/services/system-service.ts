import { apiGet, apiPost } from './http-api-client'

export interface SystemInfo {
  os: string
  exit_supported: boolean
}

export async function getSystemInfo(): Promise<SystemInfo> {
  return await apiGet<SystemInfo>('/api/system/info')
}

export async function exitMediaGen(): Promise<void> {
  await apiPost<{ ok: true }>('/api/system/exit')
}

import { apiDelete, apiGet, apiPost } from '@/shared/services/http-api-client'
export {
  createProxy,
  deleteProxy,
  fetchProxies,
  testProxy,
  type ProxyItem,
} from '@/shared/services/proxy-service'

export interface ApiKeyItem {
  id: string
  name: string
  masked_key: string
  created_at: string
}

export interface ApiKeyCreateResult {
  id: string
  key: string
  masked_key: string
}

export interface BrowserSessionItem {
  id: string
  purpose: string
  status: string
  platform: string
  account_id?: string | null
  task_id?: string | null
  url: string
  title: string
  created_at: string
}

export async function fetchApiKeys(): Promise<ApiKeyItem[]> {
  return await apiGet<ApiKeyItem[]>('/api/api-keys')
}

export async function createApiKey(name: string): Promise<ApiKeyCreateResult> {
  return await apiPost<ApiKeyCreateResult>('/api/api-keys', { name })
}

export async function deleteApiKey(id: string): Promise<void> {
  await apiDelete(`/api/api-keys/${encodeURIComponent(id)}`)
}

export async function fetchBrowserSessions(): Promise<BrowserSessionItem[]> {
  return await apiGet<BrowserSessionItem[]>('/api/browser-sessions')
}

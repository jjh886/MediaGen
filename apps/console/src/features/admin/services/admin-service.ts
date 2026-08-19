import { apiDelete, apiGet, apiPost } from '@/shared/services/http-api-client'
import { LIST_PAGE_SIZE, pagedQuery } from '@/shared/services/list-pagination'
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

export async function fetchApiKeys(limit = LIST_PAGE_SIZE, offset = 0): Promise<ApiKeyItem[]> {
  return await apiGet<ApiKeyItem[]>(`/api/api-keys?${pagedQuery(limit, offset)}`)
}

export async function createApiKey(name: string): Promise<ApiKeyCreateResult> {
  return await apiPost<ApiKeyCreateResult>('/api/api-keys', { name })
}

export async function deleteApiKey(id: string): Promise<void> {
  await apiDelete(`/api/api-keys/${encodeURIComponent(id)}`)
}

export async function fetchBrowserSessions(
  limit = LIST_PAGE_SIZE,
  offset = 0,
  taskId = ''
): Promise<BrowserSessionItem[]> {
  const params = pagedQuery(limit, offset)
  params.set('takeover_only', 'true')
  if (taskId) params.set('task_id', taskId)
  return await apiGet<BrowserSessionItem[]>(`/api/browser-sessions?${params}`)
}

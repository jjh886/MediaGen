import { apiDelete, apiGet, apiPost, apiPatch } from './http-api-client'
import { LIST_PAGE_SIZE, pagedQuery } from './list-pagination'

export interface ProxyItem {
  id: string
  name: string
  server: string
  username: string
  password_configured: boolean
  enabled: boolean
  created_at: string
  bound_account_count?: number
}

export interface ProxyCreatePayload {
  name: string
  server: string
  username: string
  password: string
}

export interface ProxyTestResult {
  ok: boolean
  status: number
  url: string
}

export async function fetchProxies(limit = LIST_PAGE_SIZE, offset = 0): Promise<ProxyItem[]> {
  return await apiGet<ProxyItem[]>(`/api/proxies?${pagedQuery(limit, offset)}`)
}

export async function fetchAllProxies(): Promise<ProxyItem[]> {
  const proxies: ProxyItem[] = []
  while (true) {
    const page = await fetchProxies(100, proxies.length)
    proxies.push(...page)
    if (page.length < 100) return proxies
  }
}

export async function createProxy(payload: ProxyCreatePayload): Promise<void> {
  await apiPost('/api/proxies', { ...payload, enabled: true })
}

export async function deleteProxy(id: string): Promise<void> {
  await apiDelete(`/api/proxies/${encodeURIComponent(id)}`)
}

export async function updateProxy(
  id: string,
  payload: Partial<ProxyCreatePayload> & { enabled?: boolean }
): Promise<void> {
  await apiPatch(`/api/proxies/${encodeURIComponent(id)}`, payload)
}

export function proxyOptionLabel(proxy: ProxyItem): string {
  return `${proxy.name || proxy.server} · 已绑定 ${proxy.bound_account_count ?? 0} 个账号`
}

export async function testProxy(id: string): Promise<ProxyTestResult> {
  return await apiPost<ProxyTestResult>(`/api/proxies/${encodeURIComponent(id)}/test`, {})
}

import { apiDelete, apiGet, apiPost } from './http-api-client'

export interface ProxyItem {
  id: string
  name: string
  server: string
  username: string
  password_configured: boolean
  enabled: boolean
  created_at: string
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

export async function fetchProxies(): Promise<ProxyItem[]> {
  return await apiGet<ProxyItem[]>('/api/proxies')
}

export async function createProxy(payload: ProxyCreatePayload): Promise<void> {
  await apiPost('/api/proxies', { ...payload, enabled: true })
}

export async function deleteProxy(id: string): Promise<void> {
  await apiDelete(`/api/proxies/${encodeURIComponent(id)}`)
}

export async function testProxy(id: string): Promise<ProxyTestResult> {
  return await apiPost<ProxyTestResult>(`/api/proxies/${encodeURIComponent(id)}/test`, {})
}

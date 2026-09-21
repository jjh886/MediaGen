import { apiGet, apiPost, apiPatch, apiDelete } from './http-api-client'
export interface AdminUser {
  id: string
  username: string
  display_name: string
  enabled: boolean
  created_at: string
  updated_at: string
  last_login_at: string | null
}
export const fetchAdminUsers = () => apiGet<AdminUser[]>('/api/admin-users')
export const createAdminUser = (body: {
  username: string
  display_name: string
  password: string
}) => apiPost<AdminUser>('/api/admin-users', body)
export const updateAdminUser = (
  id: string,
  body: Partial<Pick<AdminUser, 'username' | 'display_name' | 'enabled'>>
) => apiPatch<AdminUser>(`/api/admin-users/${encodeURIComponent(id)}`, body)
export const deleteAdminUser = (id: string) =>
  apiDelete(`/api/admin-users/${encodeURIComponent(id)}`)
export const resetAdminPassword = (id: string, password: string) =>
  apiPost(`/api/admin-users/${encodeURIComponent(id)}/password`, { password })

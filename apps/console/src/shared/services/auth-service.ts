import { apiGet, apiPost } from './http-api-client'

export interface AuthStatus {
  supported: boolean
  configured: boolean
  authenticated: boolean
  username?: string
  user_id?: string
}

export interface LoginPayload {
  username: string
  password: string
}

export async function getAuthStatus(): Promise<AuthStatus> {
  return await apiGet<AuthStatus>('/api/auth/status')
}

export async function loginAdmin(payload: LoginPayload): Promise<AuthStatus> {
  return await apiPost<AuthStatus>('/api/auth/login', payload)
}

export async function logoutAdmin(): Promise<void> {
  await apiPost<{ ok: true }>('/api/auth/logout')
}

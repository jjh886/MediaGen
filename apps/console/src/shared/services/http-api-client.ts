const API_BASE = import.meta.env.VITE_MEDIAGEN_API_BASE || ''
const ADMIN_TOKEN_STORAGE_KEY = 'mediagen_admin_token'

export class HttpApiError extends Error {
  constructor(
    message: string,
    public readonly status: number
  ) {
    super(message)
  }
}

function apiUrl(path: string): string {
  if (!API_BASE) return path
  return new URL(path, API_BASE).toString()
}

function requestInit(init: RequestInit): RequestInit {
  return API_BASE ? { ...init, credentials: 'include' } : init
}

async function parseResponseBody(response: Response): Promise<unknown> {
  const text = await response.text()
  if (!text) return null
  try {
    return JSON.parse(text) as unknown
  } catch {
    return text
  }
}

function errorMessage(payload: unknown, status: number): string {
  if (payload && typeof payload === 'object' && 'error' in payload) {
    return String((payload as { error: unknown }).error)
  }
  if (typeof payload === 'string' && payload.trim()) {
    return payload.trim()
  }
  return `HTTP ${status}`
}

function jsonHeaders(): HeadersInit {
  return {
    Accept: 'application/json',
    ...adminTokenHeader(),
  }
}

function jsonPostHeaders(): HeadersInit {
  return {
    ...jsonHeaders(),
    'Content-Type': 'application/json',
  }
}

function adminTokenHeader(): HeadersInit {
  const token = currentAdminToken()
  return token ? { 'X-MediaGen-Admin-Token': token } : {}
}

function currentAdminToken(): string {
  if (typeof window === 'undefined') return ''
  const tokenFromUrl = new URLSearchParams(window.location.search).get('admin_token')?.trim() || ''
  if (tokenFromUrl) {
    window.localStorage.setItem(ADMIN_TOKEN_STORAGE_KEY, tokenFromUrl)
    removeAdminTokenFromUrl()
    return tokenFromUrl
  }
  return window.localStorage.getItem(ADMIN_TOKEN_STORAGE_KEY)?.trim() || ''
}

function removeAdminTokenFromUrl() {
  const url = new URL(window.location.href)
  url.searchParams.delete('admin_token')
  window.history.replaceState(null, '', url.toString())
}

function unwrapData<T>(payload: unknown): T {
  if (payload && typeof payload === 'object' && 'data' in payload) {
    return (payload as { data: T }).data
  }
  return payload as T
}

export async function apiGet<T>(path: string): Promise<T> {
  const response = await fetch(
    apiUrl(path),
    requestInit({
      headers: jsonHeaders(),
    })
  )
  const payload = await parseResponseBody(response)
  if (!response.ok) {
    if (response.status === 401 && !path.startsWith('/api/auth/'))
      window.dispatchEvent(new Event('mediagen:auth-expired'))
    throw new HttpApiError(errorMessage(payload, response.status), response.status)
  }
  return unwrapData<T>(payload)
}

export async function apiPost<T>(
  path: string,
  body?: unknown,
  method: 'POST' | 'PATCH' = 'POST'
): Promise<T> {
  const response = await fetch(
    apiUrl(path),
    requestInit({
      method,
      headers: {
        ...jsonPostHeaders(),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
  )
  const payload = await parseResponseBody(response)
  if (!response.ok) {
    if (response.status === 401 && !path.startsWith('/api/auth/'))
      window.dispatchEvent(new Event('mediagen:auth-expired'))
    throw new HttpApiError(errorMessage(payload, response.status), response.status)
  }
  return unwrapData<T>(payload)
}

export function apiPatch<T>(path: string, body: unknown): Promise<T> {
  return apiPost<T>(path, body, 'PATCH')
}

export async function apiDelete<T>(path: string): Promise<T> {
  const response = await fetch(
    apiUrl(path),
    requestInit({
      method: 'DELETE',
      headers: jsonHeaders(),
    })
  )
  const payload = await parseResponseBody(response)
  if (!response.ok) {
    if (response.status === 401 && !path.startsWith('/api/auth/'))
      window.dispatchEvent(new Event('mediagen:auth-expired'))
    throw new HttpApiError(errorMessage(payload, response.status), response.status)
  }
  return unwrapData<T>(payload)
}

export function apiEventUrl(path: string): string {
  return apiUrl(path)
}

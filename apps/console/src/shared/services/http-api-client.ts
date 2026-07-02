const API_BASE = import.meta.env.VITE_MEDIAGEN_API_BASE || ''

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

async function parseJson(response: Response): Promise<unknown> {
  const text = await response.text()
  if (!text) return null
  return JSON.parse(text) as unknown
}

function unwrapData<T>(payload: unknown): T {
  if (payload && typeof payload === 'object' && 'data' in payload) {
    return (payload as { data: T }).data
  }
  return payload as T
}

export async function apiGet<T>(path: string): Promise<T> {
  const response = await fetch(apiUrl(path), {
    headers: { Accept: 'application/json' },
  })
  const payload = await parseJson(response)
  if (!response.ok) {
    const message =
      payload && typeof payload === 'object' && 'error' in payload
        ? String((payload as { error: unknown }).error)
        : `HTTP ${response.status}`
    throw new HttpApiError(message, response.status)
  }
  return unwrapData<T>(payload)
}

export async function apiPost<T>(path: string, body?: unknown): Promise<T> {
  const response = await fetch(apiUrl(path), {
    method: 'POST',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const payload = await parseJson(response)
  if (!response.ok) {
    const message =
      payload && typeof payload === 'object' && 'error' in payload
        ? String((payload as { error: unknown }).error)
        : `HTTP ${response.status}`
    throw new HttpApiError(message, response.status)
  }
  return unwrapData<T>(payload)
}

export async function apiDelete<T>(path: string): Promise<T> {
  const response = await fetch(apiUrl(path), {
    method: 'DELETE',
    headers: { Accept: 'application/json' },
  })
  const payload = await parseJson(response)
  if (!response.ok) {
    const message =
      payload && typeof payload === 'object' && 'error' in payload
        ? String((payload as { error: unknown }).error)
        : `HTTP ${response.status}`
    throw new HttpApiError(message, response.status)
  }
  return unwrapData<T>(payload)
}

export function apiEventUrl(path: string): string {
  return apiUrl(path)
}

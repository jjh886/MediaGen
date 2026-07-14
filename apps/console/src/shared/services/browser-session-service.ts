import { apiEventUrl, apiGet, apiPost } from './http-api-client'

export type BrowserSessionPurpose =
  | 'add_account'
  | 'relogin_account'
  | 'human_verification'
  | 'task_takeover'
  | 'debug_preview'

export type BrowserSessionStatus = 'starting' | 'active' | 'completed' | 'cancelled' | 'failed'

export interface BrowserSession {
  id: string
  purpose: BrowserSessionPurpose
  status: BrowserSessionStatus
  platform: string
  account_id?: string | null
  task_id?: string | null
  proxy_id?: string | null
  start_url: string
  url: string
  title: string
  target_id: string
  headless: boolean
  browser_name: string
  runtime_id: string
  viewport: { width: number; height: number }
  device_scale_factor: number
  message: string
  cookie_count: number
  verification_reason?: string | null
  profile_dir: string
  created_at: string
  updated_at: string
  cancelled_at?: string | null
  completed_at?: string | null
}

export interface BrowserSessionCreateRequest {
  purpose: BrowserSessionPurpose
  platform: string
  start_url: string
  account_id?: string | null
  task_id?: string | null
  proxy_id?: string | null
  verification_reason?: string | null
  profile_dir?: string | null
  metadata?: Record<string, unknown>
  width?: number
  height?: number
  deviceScaleFactor?: number
}

export type BrowserInputRequest =
  | {
      kind: 'mouse'
      event: 'move' | 'down' | 'up'
      x: number
      y: number
      button?: string
      buttons?: number
      clickCount?: number
      modifiers?: number
      readCursor?: boolean
    }
  | {
      kind: 'wheel'
      x: number
      y: number
      deltaX: number
      deltaY: number
      modifiers?: number
      readCursor?: boolean
    }
  | {
      kind: 'key'
      event: 'down' | 'up'
      key: string
      code?: string
      modifiers?: number
      ctrlKey?: boolean
      metaKey?: boolean
      altKey?: boolean
      shiftKey?: boolean
      text?: string
      unmodifiedText?: string
      autoRepeat?: boolean
      isKeypad?: boolean
      location?: number
      commands?: string[]
    }
  | {
      kind: 'ime'
      event: 'set_composition' | 'commit' | 'cancel'
      text?: string
      selectionStart?: number
      selectionEnd?: number
    }

export interface BrowserPointerState {
  cursor: string
  editable: boolean
  selectable: boolean
  has_selection: boolean
  password: boolean
  element_kind: string
}

export interface BrowserInputResponse {
  ok: boolean
  cursor: string
  pointer: BrowserPointerState
}

export type BrowserHistoryAction = 'back' | 'forward' | 'reload'

interface SessionEnvelope {
  ok: boolean
  session: BrowserSession
}

export async function createBrowserSession(
  request: BrowserSessionCreateRequest
): Promise<BrowserSession> {
  const response = await apiPost<SessionEnvelope>('/api/browser-sessions', request)
  return response.session
}

export async function completeBrowserSession(id: string): Promise<BrowserSession> {
  const response = await apiPost<SessionEnvelope>(`/api/browser-sessions/${id}/complete`)
  return response.session
}

export async function cancelBrowserSession(id: string): Promise<void> {
  await apiPost(`/api/browser-sessions/${id}/cancel`)
}

export async function resizeBrowserSession(
  id: string,
  width: number,
  height: number,
  deviceScaleFactor?: number
): Promise<void> {
  await apiPost(`/api/browser-sessions/${id}/resize`, { width, height, deviceScaleFactor })
}

export async function navigateBrowserSession(id: string, url: string): Promise<void> {
  await apiPost(`/api/browser-sessions/${id}/navigate`, { url })
}

export async function browserSessionHistory(
  id: string,
  action: BrowserHistoryAction
): Promise<void> {
  await apiPost(`/api/browser-sessions/${id}/history`, { action })
}

export async function sendBrowserInput(
  id: string,
  input: BrowserInputRequest
): Promise<BrowserInputResponse> {
  return await apiPost<BrowserInputResponse>(`/api/browser-sessions/${id}/input`, input)
}

export async function writeBrowserClipboard(id: string, text: string): Promise<void> {
  await apiPost(`/api/browser-sessions/${id}/clipboard`, { action: 'insert_text', text })
}

export async function copyBrowserSelection(id: string): Promise<string> {
  const response = await apiPost<{ ok: boolean; text: string }>(
    `/api/browser-sessions/${id}/clipboard`,
    { action: 'copy_selection' }
  )
  return response.text
}

export async function cutBrowserSelection(id: string): Promise<string> {
  const response = await apiPost<{ ok: boolean; text: string }>(
    `/api/browser-sessions/${id}/clipboard`,
    { action: 'cut_selection' }
  )
  return response.text
}

export async function selectAllBrowserContent(id: string): Promise<void> {
  await apiPost(`/api/browser-sessions/${id}/clipboard`, { action: 'select_all' })
}

export async function fetchBrowserSession(id: string): Promise<BrowserSession> {
  const response = await apiGet<SessionEnvelope>(`/api/browser-sessions/${id}`)
  return response.session
}

export function browserSessionEventsUrl(id: string): string {
  return apiEventUrl(`/api/browser-sessions/${id}/events`)
}

export function browserSessionStreamUrl(id: string): string {
  const url = new URL(apiEventUrl(`/api/browser-sessions/${id}/stream`), window.location.href)
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  return url.toString()
}

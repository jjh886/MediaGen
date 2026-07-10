export type LoginState =
  | 'available'
  | 'daily_quota_exhausted'
  | 'total_quota_exhausted'
  | 'login_expired'
  | 'verification_required'
  | 'cooldown'
  | 'disabled'

export interface Account {
  id: string
  platform: string
  name: string
  email: string
  avatar_url: string
  tier: string
  login_state: LoginState
  cooldown_until?: string | null
  used_today: number
  daily_limit: number
  used_total: number
  total_limit: number
  concurrency_limit: number
  task_interval_seconds: number
  proxy_url?: string
  proxy_id?: string
  browser_os?: string
  browser_version?: string
  api_key?: string
  api_key_masked?: string
  api_key_configured?: boolean
  profile_dir?: string
  created_at: string
}

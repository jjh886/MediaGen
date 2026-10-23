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
  user_id?: string
  priority?: number
  text_concurrency_limit?: number
  text_concurrency_default?: number
  text_daily_limit?: number
  text_total_limit?: number
  text_used_today?: number
  text_used_total?: number
  quota_task_kind?: string | null
  quota_holds?: Record<string, { state: string; until: string | null }>
  quota_windows?: {
    name: string
    task_kind: string
    remaining_percent?: number | null
    remaining_count?: number | null
    unit?: string
    reset_at?: string | null
  }[]
  login_state_reason?: string
  last_check_at?: string | null
  last_success_at?: string | null
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
  browser_profile_dir?: string
  created_at: string
}

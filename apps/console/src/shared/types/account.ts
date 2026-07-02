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
  used_today: number
  daily_limit: number
  used_total: number
  total_limit: number
  concurrency_limit: number
  task_interval_seconds: number
  group_name?: string
  proxy_url?: string
  api_key?: string
  profile_dir?: string
  created_at: string
}

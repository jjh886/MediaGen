import type { Account, LoginState } from '../types/account'

export const accountStateVariant: Record<LoginState, 'success' | 'warning' | 'error' | 'disabled'> =
  {
    available: 'success',
    daily_quota_exhausted: 'warning',
    cooldown: 'warning',
    login_expired: 'error',
    total_quota_exhausted: 'error',
    verification_required: 'error',
    disabled: 'disabled',
  }

export const accountStateLabel: Record<LoginState, string> = {
  available: '可用',
  daily_quota_exhausted: '额度已满',
  cooldown: '冷却中',
  login_expired: '登录过期',
  total_quota_exhausted: '总量已满',
  verification_required: '需验证',
  disabled: '已停用',
}

export function accountResetText(account: Pick<Account, 'login_state' | 'cooldown_until'>): string {
  if (!account.cooldown_until) return ''
  if (!['cooldown', 'daily_quota_exhausted'].includes(account.login_state)) return ''
  const date = new Date(account.cooldown_until)
  if (Number.isNaN(date.getTime())) return ''
  return `额度恢复时间 ${formatAccountDate(date)}`
}

function formatAccountDate(date: Date): string {
  return new Intl.DateTimeFormat(undefined, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date)
}

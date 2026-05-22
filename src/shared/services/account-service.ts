import { tauriInvoke } from '@/shared/services/tauri-bridge'
import type { Account } from '@/shared/types/account'

const MOCK_ACCOUNTS: Account[] = [
  {
    id: 'acc-003',
    platform: 'flow',
    name: 'Flow账号',
    email: 'flow@example.com',
    avatar_url: '',
    tier: 'premium',
    login_state: 'available',
    used_today: 5,
    daily_limit: 100,
    used_total: 80,
    total_limit: 5000,
    concurrency_limit: 5,
    created_at: '2026-02-20T00:00:00Z',
  },
  {
    id: 'acc-004',
    platform: 'doubao',
    name: '豆包账号',
    email: 'doubao@example.com',
    avatar_url: '',
    tier: 'standard',
    login_state: 'login_expired',
    used_today: 0,
    daily_limit: 30,
    used_total: 200,
    total_limit: 2000,
    concurrency_limit: 2,
    created_at: '2026-01-10T00:00:00Z',
  },
]

const isTauri = '__TAURI_INTERNALS__' in window

function mockByPlatform(platform?: string): Account[] {
  return platform ? MOCK_ACCOUNTS.filter((a) => a.platform === platform) : MOCK_ACCOUNTS
}

export async function fetchAccounts(platform?: string): Promise<Account[]> {
  if (!isTauri) return mockByPlatform(platform)
  try {
    return (await tauriInvoke<Account[]>('get_accounts', { platform: platform ?? '' })) ?? []
  } catch {
    console.warn('[account-service] get_accounts 调用失败')
    return []
  }
}

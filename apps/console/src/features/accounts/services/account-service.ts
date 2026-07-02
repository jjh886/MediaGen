import { fetchAccounts as fetchSharedAccounts } from '@/shared/services/account-service'
import {
  createBrowserSession,
  type BrowserSession,
} from '@/shared/services/browser-session-service'
import { apiDelete, apiGet, apiPost } from '@/shared/services/http-api-client'
import type { Account } from '@/shared/types/account'

export { fetchAccounts } from '@/shared/services/account-service'

interface PluginLoginInfo {
  platform_id: string
  login_url?: string
  workspace_url?: string
}

export type AccountConfigUpdate = Partial<
  Pick<
    Account,
    | 'tier'
    | 'concurrency_limit'
    | 'daily_limit'
    | 'total_limit'
    | 'task_interval_seconds'
    | 'group_name'
    | 'proxy_url'
    | 'api_key'
  >
>

export async function updateAccount(id: string, updates: AccountConfigUpdate): Promise<void> {
  await apiPost(`/api/accounts/${encodeURIComponent(id)}`, updates)
}

export async function deleteAccount(id: string): Promise<void> {
  await apiDelete(`/api/accounts/${encodeURIComponent(id)}`)
}

export async function toggleAccount(id: string, disabled: boolean): Promise<void> {
  await apiPost(`/api/accounts/${encodeURIComponent(id)}/toggle`, { disabled })
}

export async function reloginAccount(id: string): Promise<void> {
  const accounts = await fetchSharedAccounts()
  const account = accounts.find((item) => item.id === id)
  if (!account) throw new Error('账号不存在，请刷新后重试。')
  await startAccountLoginSession(account.platform, id)
}

export async function startAccountLoginSession(
  platform: string,
  accountId?: string | null,
  proxyUrl?: string
): Promise<BrowserSession> {
  const plugin = await resolvePluginLoginInfo(platform)
  const account = accountId
    ? (await fetchSharedAccounts(platform)).find((item) => item.id === accountId)
    : null
  const resolvedProxy = proxyUrl === undefined ? account?.proxy_url?.trim() || '' : proxyUrl.trim()
  const metadata = resolvedProxy ? { proxy_server: resolvedProxy } : undefined
  return createBrowserSession({
    purpose: accountId ? 'relogin_account' : 'add_account',
    platform,
    account_id: accountId || null,
    start_url: plugin.login_url || plugin.workspace_url || 'https://example.com',
    profile_dir: account?.profile_dir || null,
    metadata,
    width: 1365,
    height: 768,
    deviceScaleFactor: browserDeviceScaleFactor(),
  })
}

function browserDeviceScaleFactor(): number {
  if (typeof window === 'undefined') return 1
  return Math.min(2, Math.max(1, window.devicePixelRatio || 1))
}

export async function addAccountByLogin(platform: string): Promise<Account | null> {
  await startAccountLoginSession(platform)
  return null
}

export async function checkAccountsLogin(): Promise<{ checked: number; expired: number }> {
  return await apiPost<{ checked: number; expired: number }>('/api/accounts/check-login')
}

export async function batchRegister(
  platform: string,
  count: number,
  password: string,
  birthday: string
): Promise<void> {
  await apiPost('/api/accounts/batch-register', { platform, count, password, birthday })
}

async function resolvePluginLoginInfo(platform: string): Promise<PluginLoginInfo> {
  try {
    const plugins = await apiGet<PluginLoginInfo[]>('/api/plugins')
    return plugins.find((plugin) => plugin.platform_id === platform) ?? { platform_id: platform }
  } catch {
    return { platform_id: platform }
  }
}

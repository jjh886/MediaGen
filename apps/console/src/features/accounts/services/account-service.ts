import { fetchAccounts as fetchSharedAccounts } from '@/shared/services/account-service'
import {
  createBrowserSession,
  type BrowserSession,
} from '@/shared/services/browser-session-service'
import { apiDelete, apiGet, apiPost } from '@/shared/services/http-api-client'
import type { Account } from '@/shared/types/account'
import type { BrowserProfilePayload } from '../utils/browser-profile'

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
    | 'proxy_url'
    | 'proxy_id'
    | 'browser_os'
    | 'browser_version'
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
  proxyUrl?: string,
  proxyId?: string,
  browserProfile?: BrowserProfilePayload,
  proxyMode?: 'direct'
): Promise<BrowserSession> {
  const plugin = await resolvePluginLoginInfo(platform)
  const account = accountId
    ? (await fetchSharedAccounts(platform)).find((item) => item.id === accountId)
    : null
  const resolvedProxyId =
    proxyMode === 'direct'
      ? ''
      : proxyId === undefined && accountId
        ? account?.proxy_id?.trim()
        : proxyId
  const resolvedProxy = resolvedProxyId
    ? ''
    : proxyMode === 'direct'
      ? ''
      : proxyUrl === undefined
        ? account?.proxy_url?.trim() || ''
        : proxyUrl.trim()
  const resolvedBrowserOs =
    browserProfile === undefined ? account?.browser_os?.trim() || '' : browserProfile.browser_os
  const resolvedBrowserVersion =
    browserProfile === undefined
      ? account?.browser_version?.trim() || ''
      : browserProfile.browser_version
  const metadata = browserSessionMetadata(
    resolvedProxy,
    resolvedBrowserOs.trim(),
    resolvedBrowserVersion.trim(),
    proxyMode
  )
  return createBrowserSession({
    purpose: accountId ? 'relogin_account' : 'add_account',
    platform,
    account_id: accountId || null,
    start_url: plugin.login_url || plugin.workspace_url || 'https://example.com',
    // 重新登录必须使用新目录；旧目录可能仍被过期会话或常驻浏览器占用。
    profile_dir: null,
    proxy_id: resolvedProxyId || null,
    metadata,
    width: 1365,
    height: 768,
    deviceScaleFactor: browserDeviceScaleFactor(),
  })
}

function browserSessionMetadata(
  proxyUrl: string,
  browserOs: string,
  browserVersion: string,
  proxyMode?: 'direct'
) {
  const metadata: Record<string, string> = {}
  if (proxyMode === 'direct') metadata.proxy_mode = 'direct'
  if (proxyUrl) metadata.proxy_server = proxyUrl
  if (browserOs && browserVersion) {
    metadata.browser_os = browserOs
    metadata.browser_version = browserVersion
  }
  return Object.keys(metadata).length > 0 ? metadata : undefined
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

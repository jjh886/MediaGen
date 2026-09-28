import { fetchAccount } from '@/shared/services/account-service'
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
    | 'priority'
    | 'text_concurrency_limit'
    | 'text_daily_limit'
    | 'text_total_limit'
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

export async function updateAccount(id: string, updates: AccountConfigUpdate): Promise<Account> {
  return await apiPost<Account>(`/api/accounts/${encodeURIComponent(id)}`, updates)
}

export async function refreshAccountQuota(id: string): Promise<Account> {
  return await apiPost<Account>(`/api/accounts/${encodeURIComponent(id)}/refresh-quota`)
}

export async function deleteAccount(id: string): Promise<void> {
  await apiDelete(`/api/accounts/${encodeURIComponent(id)}`)
}

export async function toggleAccount(id: string, disabled: boolean): Promise<Account> {
  return await apiPost<Account>(`/api/accounts/${encodeURIComponent(id)}/toggle`, { disabled })
}

export async function reloginAccount(id: string): Promise<void> {
  const account = await fetchAccount(id)
  await startAccountLoginSession(account.platform, id)
}

export async function startAccountLoginSession(
  platform: string,
  accountId?: string | null,
  proxyUrl?: string,
  proxyId?: string,
  browserProfile?: BrowserProfilePayload,
  proxyMode?: 'direct',
  reuseBrowserProfileAccountId?: string
): Promise<BrowserSession> {
  const plugin = await resolvePluginLoginInfo(platform)
  const account = accountId ? await fetchAccount(accountId) : null
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
    // 重新登录沿用账号目录，保留 Cookie、本地存储和已有浏览器资料。
    profile_dir: account?.profile_dir || null,
    reuse_browser_profile_account_id: reuseBrowserProfileAccountId || null,
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

export async function checkAccountsLogin(): Promise<{
  checked: number
  expired: number
  limited: number
}> {
  return await apiPost<{ checked: number; expired: number; limited: number }>(
    '/api/accounts/check-login'
  )
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

export interface AccountModelCatalog {
  models: Array<{
    id: string
    display_name: string
    task_kind: string
    capabilities: {
      input_modalities?: string[]
      output_modalities?: string[]
      endpoints?: string[]
      streaming?: boolean
      tool_calling?: boolean
      input_schema?: Record<string, unknown>
      unsupported_parameters?: string[]
    }
  }>
  refreshed_at: number | null
  refresh_error: string | null
  discovery_supported: boolean
}

export function fetchAccountModels(id: string, refresh = false): Promise<AccountModelCatalog> {
  return apiGet<AccountModelCatalog>(
    `/api/accounts/${encodeURIComponent(id)}/models?refresh=${refresh}`
  )
}

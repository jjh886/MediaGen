import { apiGet } from '@/shared/services/http-api-client'
import { LIST_PAGE_SIZE, pagedQuery } from '@/shared/services/list-pagination'
import type { Account } from '@/shared/types/account'

export interface AccountSummary {
  available: number
  total: number
}

export async function fetchAccounts(
  platform = '',
  limit = LIST_PAGE_SIZE,
  offset = 0
): Promise<Account[]> {
  const params = pagedQuery(limit, offset)
  if (platform) params.set('platform', platform)
  return await apiGet<Account[]>(`/api/accounts?${params}`)
}

export async function fetchAllAccounts(platform = ''): Promise<Account[]> {
  const accounts: Account[] = []
  while (true) {
    const page = await fetchAccounts(platform, 100, accounts.length)
    accounts.push(...page)
    if (page.length < 100) return accounts
  }
}

export async function fetchAccount(id: string): Promise<Account> {
  return await apiGet<Account>(`/api/accounts/${encodeURIComponent(id)}`)
}

export async function fetchAccountSummary(): Promise<AccountSummary> {
  return await apiGet<AccountSummary>('/api/accounts/summary')
}

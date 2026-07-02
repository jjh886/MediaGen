import { apiGet } from '@/shared/services/http-api-client'
import type { Account } from '@/shared/types/account'

export async function fetchAccounts(platform?: string): Promise<Account[]> {
  const query = platform ? `?platform=${encodeURIComponent(platform)}` : ''
  return await apiGet<Account[]>(`/api/accounts${query}`)
}

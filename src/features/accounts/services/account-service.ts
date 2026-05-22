import { tauriInvoke } from '@/shared/services/tauri-bridge'
import type { Account } from '@/shared/types/account'

export { fetchAccounts } from '@/shared/services/account-service'

const isTauri = '__TAURI_INTERNALS__' in window

export async function updateAccount(
  id: string,
  updates: Partial<Pick<Account, 'tier' | 'concurrency_limit' | 'daily_limit' | 'total_limit'>>
): Promise<void> {
  if (!isTauri) return
  // Tauri v2 默认把 Rust 的 snake_case 参数暴露成 camelCase，必须用 camelCase 键，否则 Option<T> 会静默回落为 None
  const payload: Record<string, unknown> = { id }
  if (updates.tier !== undefined) payload.tier = updates.tier
  if (updates.concurrency_limit !== undefined) payload.concurrencyLimit = updates.concurrency_limit
  if (updates.daily_limit !== undefined) payload.dailyLimit = updates.daily_limit
  if (updates.total_limit !== undefined) payload.totalLimit = updates.total_limit
  await tauriInvoke('update_account', payload)
}

export async function deleteAccount(id: string): Promise<void> {
  if (!isTauri) return
  await tauriInvoke('delete_account', { id })
}

export async function toggleAccount(id: string, disabled: boolean): Promise<void> {
  if (!isTauri) return
  await tauriInvoke('toggle_account', { id, disabled })
}

export async function reloginAccount(id: string): Promise<void> {
  if (!isTauri) return
  try {
    await tauriInvoke('relogin_account', { id })
  } catch (e) {
    console.warn('[account-service] relogin_account 调用失败:', e)
    throw e
  }
}

export async function addAccountByLogin(platform: string): Promise<Account | null> {
  if (!isTauri) return null
  try {
    return await tauriInvoke<Account>('add_account_start', { platform })
  } catch (e) {
    console.warn('[account-service] add_account_start 调用失败:', e)
    throw e
  }
}

export async function checkAccountsLogin(): Promise<{ checked: number; expired: number }> {
  if (!isTauri) return { checked: 0, expired: 0 }
  try {
    return (
      (await tauriInvoke<{ checked: number; expired: number }>('check_accounts_login')) ?? {
        checked: 0,
        expired: 0,
      }
    )
  } catch {
    console.warn('[account-service] check_accounts_login 调用失败')
    return { checked: 0, expired: 0 }
  }
}

export async function batchRegister(
  platform: string,
  count: number,
  password: string,
  birthday: string
): Promise<void> {
  if (!isTauri) return
  await tauriInvoke('batch_register', { platform, count, password, birthday })
}

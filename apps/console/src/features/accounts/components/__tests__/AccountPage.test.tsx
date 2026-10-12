import { render, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import AccountPage from '../AccountPage'

const store = vi.hoisted(() => ({
  accounts: [],
  activePlatform: '',
  loading: false,
  loadingMore: false,
  hasMore: false,
  error: '',
  checking: false,
  fetch: vi.fn(async () => undefined),
  loadMore: vi.fn(async () => undefined),
  syncLatest: vi.fn(async () => undefined),
  setActivePlatform: vi.fn(),
  remove: vi.fn(async () => undefined),
  toggle: vi.fn(async () => undefined),
  update: vi.fn(async () => undefined),
  checkLogin: vi.fn(async () => ({ checked: 0, expired: 0, limited: 0 })),
}))

vi.mock('@/shared/services/proxy-service', () => ({ fetchAllProxies: vi.fn(async () => []) }))

vi.mock('../../hooks/useAccountStore', () => ({ useAccountStore: () => store }))
vi.mock('@/shared/services/plugin-service', () => ({
  fetchPlugins: vi.fn(async () => [
    {
      platform_id: 'codex',
      display_name: 'Codex',
      adapter_mode: 'browser',
      models: [],
      tiers: {},
      model_count: 0,
      supports_registration: false,
    },
  ]),
}))

describe('AccountPage', () => {
  it('打开页面只读取列表，不再触发真实账号检查', async () => {
    render(<AccountPage />)

    await waitFor(() => expect(store.setActivePlatform).toHaveBeenCalledWith('codex'))
    expect(store.checkLogin).not.toHaveBeenCalled()
  })
})

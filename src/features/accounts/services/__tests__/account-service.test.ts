import { describe, it, expect, vi, beforeEach } from 'vitest'

const mockInvoke = vi.fn()

vi.mock('@/shared/services/tauri-bridge', () => ({
  tauriInvoke: (...args: unknown[]) => mockInvoke(...args),
}))

// 默认测试环境下 window 没有 __TAURI_INTERNALS__，走非 Tauri 分支（mock 数据）
// 要测 Tauri 分支需在模块加载前设置 window.__TAURI_INTERNALS__

describe('account-service (非 Tauri 环境)', () => {
  let fetchAccounts: typeof import('../account-service').fetchAccounts
  let updateAccount: typeof import('../account-service').updateAccount
  let deleteAccount: typeof import('../account-service').deleteAccount
  let toggleAccount: typeof import('../account-service').toggleAccount
  let reloginAccount: typeof import('../account-service').reloginAccount
  let addAccountByLogin: typeof import('../account-service').addAccountByLogin
  let checkAccountsLogin: typeof import('../account-service').checkAccountsLogin
  let batchRegister: typeof import('../account-service').batchRegister

  beforeEach(async () => {
    mockInvoke.mockReset()
    vi.resetModules()
    // 确保没有 __TAURI_INTERNALS__
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    delete (window as any).__TAURI_INTERNALS__
    const mod = await import('../account-service')
    fetchAccounts = mod.fetchAccounts
    updateAccount = mod.updateAccount
    deleteAccount = mod.deleteAccount
    toggleAccount = mod.toggleAccount
    reloginAccount = mod.reloginAccount
    addAccountByLogin = mod.addAccountByLogin
    checkAccountsLogin = mod.checkAccountsLogin
    batchRegister = mod.batchRegister
  })

  describe('fetchAccounts', () => {
    it('返回全部 mock 账号', async () => {
      const result = await fetchAccounts()
      expect(result).toHaveLength(2)
      expect(result[0].id).toBe('acc-003')
    })

    it('不存在的平台返回空数组', async () => {
      const result = await fetchAccounts('nonexistent')
      expect(result).toEqual([])
    })

    it('flow 平台过滤正确', async () => {
      const result = await fetchAccounts('flow')
      expect(result).toHaveLength(1)
      expect(result[0].platform).toBe('flow')
    })

    it('doubao 平台过滤正确', async () => {
      const result = await fetchAccounts('doubao')
      expect(result).toHaveLength(1)
      expect(result[0].platform).toBe('doubao')
    })
  })

  describe('updateAccount', () => {
    it('直接返回不调用 invoke', async () => {
      await expect(updateAccount('acc-001', { tier: 'pro' })).resolves.toBeUndefined()
      expect(mockInvoke).not.toHaveBeenCalled()
    })
  })

  describe('deleteAccount', () => {
    it('直接返回不调用 invoke', async () => {
      await expect(deleteAccount('acc-001')).resolves.toBeUndefined()
      expect(mockInvoke).not.toHaveBeenCalled()
    })
  })

  describe('toggleAccount', () => {
    it('直接返回不调用 invoke', async () => {
      await expect(toggleAccount('acc-001', true)).resolves.toBeUndefined()
      expect(mockInvoke).not.toHaveBeenCalled()
    })
  })

  describe('reloginAccount', () => {
    it('直接返回不调用 invoke', async () => {
      await expect(reloginAccount('acc-001')).resolves.toBeUndefined()
      expect(mockInvoke).not.toHaveBeenCalled()
    })
  })

  describe('addAccountByLogin', () => {
    it('返回 null', async () => {
      const result = await addAccountByLogin('flow')
      expect(result).toBeNull()
    })
  })

  describe('checkAccountsLogin', () => {
    it('返回 { checked: 0, expired: 0 }', async () => {
      const result = await checkAccountsLogin()
      expect(result).toEqual({ checked: 0, expired: 0 })
    })
  })

  describe('batchRegister', () => {
    it('直接返回不调用 invoke', async () => {
      await expect(batchRegister('flow', 5, 'password', '1990-01-01')).resolves.toBeUndefined()
      expect(mockInvoke).not.toHaveBeenCalled()
    })
  })
})

describe('account-service (Tauri 环境)', () => {
  let fetchAccounts: typeof import('../account-service').fetchAccounts
  let updateAccount: typeof import('../account-service').updateAccount
  let deleteAccount: typeof import('../account-service').deleteAccount
  let toggleAccount: typeof import('../account-service').toggleAccount
  let reloginAccount: typeof import('../account-service').reloginAccount
  let addAccountByLogin: typeof import('../account-service').addAccountByLogin
  let checkAccountsLogin: typeof import('../account-service').checkAccountsLogin
  let batchRegister: typeof import('../account-service').batchRegister

  beforeEach(async () => {
    mockInvoke.mockReset()
    vi.resetModules()
    // 模拟 Tauri 环境
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      value: {},
      writable: true,
      configurable: true,
    })
    const mod = await import('../account-service')
    fetchAccounts = mod.fetchAccounts
    updateAccount = mod.updateAccount
    deleteAccount = mod.deleteAccount
    toggleAccount = mod.toggleAccount
    reloginAccount = mod.reloginAccount
    addAccountByLogin = mod.addAccountByLogin
    checkAccountsLogin = mod.checkAccountsLogin
    batchRegister = mod.batchRegister
  })

  afterEach(() => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    delete (window as any).__TAURI_INTERNALS__
  })

  describe('fetchAccounts', () => {
    it('调用 get_accounts 返回数据', async () => {
      const accounts = [{ id: 'a1', platform: 'flow' }]
      mockInvoke.mockResolvedValue(accounts)

      const result = await fetchAccounts('flow')
      expect(mockInvoke).toHaveBeenCalledWith('get_accounts', { platform: 'flow' })
      expect(result).toEqual(accounts)
    })

    it('不传 platform 时传空字符串', async () => {
      mockInvoke.mockResolvedValue([])
      await fetchAccounts()
      expect(mockInvoke).toHaveBeenCalledWith('get_accounts', { platform: '' })
    })

    it('invoke 返回 null 时兜底为空数组', async () => {
      mockInvoke.mockResolvedValue(null)
      const result = await fetchAccounts()
      expect(result).toEqual([])
    })

    it('invoke 抛错时返回空数组', async () => {
      mockInvoke.mockRejectedValue(new Error('fail'))
      const result = await fetchAccounts()
      expect(result).toEqual([])
    })
  })

  describe('updateAccount', () => {
    it('调用 update_account 时把 snake_case 字段映射为 camelCase（匹配 Tauri v2 默认参数命名）', async () => {
      mockInvoke.mockResolvedValue(undefined)
      await updateAccount('acc-001', {
        tier: 'pro',
        concurrency_limit: 3,
        daily_limit: 100,
        total_limit: 1000,
        task_interval_seconds: 45,
      })
      expect(mockInvoke).toHaveBeenCalledWith('update_account', {
        id: 'acc-001',
        tier: 'pro',
        concurrencyLimit: 3,
        dailyLimit: 100,
        totalLimit: 1000,
        taskIntervalSeconds: 45,
      })
    })

    it('未传字段时不会写入 undefined 键', async () => {
      mockInvoke.mockResolvedValue(undefined)
      await updateAccount('acc-002', { tier: 'free' })
      expect(mockInvoke).toHaveBeenCalledWith('update_account', {
        id: 'acc-002',
        tier: 'free',
      })
    })
  })

  describe('deleteAccount', () => {
    it('调用 delete_account', async () => {
      mockInvoke.mockResolvedValue(undefined)
      await deleteAccount('acc-001')
      expect(mockInvoke).toHaveBeenCalledWith('delete_account', { id: 'acc-001' })
    })
  })

  describe('toggleAccount', () => {
    it('调用 toggle_account', async () => {
      mockInvoke.mockResolvedValue(undefined)
      await toggleAccount('acc-002', true)
      expect(mockInvoke).toHaveBeenCalledWith('toggle_account', {
        id: 'acc-002',
        disabled: true,
      })
    })
  })

  describe('reloginAccount', () => {
    it('调用 relogin_account', async () => {
      mockInvoke.mockResolvedValue(undefined)
      await reloginAccount('acc-003')
      expect(mockInvoke).toHaveBeenCalledWith('relogin_account', { id: 'acc-003' })
    })

    it('invoke 抛错时向外抛出', async () => {
      mockInvoke.mockRejectedValue(new Error('fail'))
      await expect(reloginAccount('acc-003')).rejects.toThrow('fail')
    })
  })

  describe('addAccountByLogin', () => {
    it('调用 add_account_start 返回账号', async () => {
      const account = { id: 'new', platform: 'flow' }
      mockInvoke.mockResolvedValue(account)
      const result = await addAccountByLogin('flow')
      expect(mockInvoke).toHaveBeenCalledWith('add_account_start', { platform: 'flow' })
      expect(result).toEqual(account)
    })

    it('invoke 抛错时向外抛出', async () => {
      mockInvoke.mockRejectedValue(new Error('fail'))
      await expect(addAccountByLogin('flow')).rejects.toThrow('fail')
    })
  })

  describe('checkAccountsLogin', () => {
    it('调用 check_accounts_login 返回结果', async () => {
      mockInvoke.mockResolvedValue({ checked: 5, expired: 2 })
      const result = await checkAccountsLogin()
      expect(mockInvoke).toHaveBeenCalledWith('check_accounts_login')
      expect(result).toEqual({ checked: 5, expired: 2 })
    })

    it('invoke 返回 null 时兜底', async () => {
      mockInvoke.mockResolvedValue(null)
      const result = await checkAccountsLogin()
      expect(result).toEqual({ checked: 0, expired: 0 })
    })

    it('invoke 抛错时返回默认值', async () => {
      mockInvoke.mockRejectedValue(new Error('fail'))
      const result = await checkAccountsLogin()
      expect(result).toEqual({ checked: 0, expired: 0 })
    })
  })

  describe('batchRegister', () => {
    it('调用 batch_register', async () => {
      mockInvoke.mockResolvedValue(undefined)
      await batchRegister('flow', 3, 'pwd123', '1995-06-15')
      expect(mockInvoke).toHaveBeenCalledWith('batch_register', {
        platform: 'flow',
        count: 3,
        password: 'pwd123',
        birthday: '1995-06-15',
      })
    })
  })
})

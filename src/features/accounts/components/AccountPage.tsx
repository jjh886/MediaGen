import { useEffect, useState } from 'react'
import { KtTabs, KtButton } from '@/shared/ui'
import { fetchPlugins, type PluginInfo } from '@/shared/services/plugin-service'
import { useAccountStore } from '../hooks/useAccountStore'
import type { Account } from '../types/account'
import AccountTable from './AccountTable'
import AccountAddDialog from './AccountAddDialog'
import AccountConfigModal from './AccountConfigModal'
import BatchRegisterDialog from './BatchRegisterDialog'

export default function AccountPage() {
  const {
    accounts,
    activePlatform,
    loading,
    checking,
    fetch,
    setActivePlatform,
    remove,
    toggle,
    update,
    checkLogin,
  } = useAccountStore()

  const [plugins, setPlugins] = useState<PluginInfo[]>([])
  const [loginOpen, setLoginOpen] = useState(false)
  const [reloginId, setReloginId] = useState<string | null>(null)
  const [configAccount, setConfigAccount] = useState<Account | null>(null)
  const [batchOpen, setBatchOpen] = useState(false)

  useEffect(() => {
    fetchPlugins().then((list) => {
      setPlugins(list)
      if (list.length > 0) {
        const first = list[0].platform_id
        if (!activePlatform) {
          setActivePlatform(first)
        } else {
          fetch()
        }
      }
    })
    // 页面加载时自动检查所有账号登录态
    checkLogin()
  }, [])

  const platformTabs = plugins.map((p) => ({ key: p.platform_id, label: p.display_name }))
  const activePlugin = plugins.find((p) => p.platform_id === activePlatform)

  const handleDelete = async (account: Account) => {
    await remove(account.id)
  }

  const handleToggle = async (account: Account) => {
    await toggle(account.id, account.login_state !== 'disabled')
  }

  const handleRelogin = (account: Account) => {
    setReloginId(account.id)
    setLoginOpen(true)
  }

  const handleAddAccount = () => {
    setReloginId(null)
    setLoginOpen(true)
  }

  const handleLoginClose = () => {
    setLoginOpen(false)
    setReloginId(null)
  }

  const handleSaveConfig = async (
    id: string,
    updates: { tier: string; concurrency_limit: number; daily_limit: number; total_limit: number }
  ) => {
    await update(id, updates)
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: 16 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <KtTabs items={platformTabs} activeKey={activePlatform} onChange={setActivePlatform} />
        <div style={{ display: 'flex', gap: 8 }}>
          <KtButton variant="default" size="sm" onClick={checkLogin} disabled={checking}>
            {checking ? '检查中...' : '检查登录态'}
          </KtButton>
          {activePlugin?.supports_registration && (
            <KtButton variant="default" size="sm" onClick={() => setBatchOpen(true)}>
              批量注册
            </KtButton>
          )}
          <KtButton variant="primary" size="sm" onClick={handleAddAccount}>
            添加账号
          </KtButton>
        </div>
      </div>

      <div style={{ flex: 1, overflow: 'auto' }}>
        {loading ? (
          <div style={{ color: 'var(--text-secondary)', padding: 32, textAlign: 'center' }}>
            加载中...
          </div>
        ) : (
          <AccountTable
            accounts={accounts}
            onConfigure={setConfigAccount}
            onRelogin={handleRelogin}
            onToggle={handleToggle}
            onDelete={handleDelete}
          />
        )}
      </div>

      <AccountAddDialog
        open={loginOpen}
        onClose={handleLoginClose}
        platform={activePlatform}
        reloginAccountId={reloginId}
        onSuccess={() => fetch()}
      />

      <AccountConfigModal
        open={configAccount !== null}
        onClose={() => setConfigAccount(null)}
        account={configAccount}
        onSave={handleSaveConfig}
      />

      <BatchRegisterDialog
        open={batchOpen}
        onClose={() => setBatchOpen(false)}
        platform={activePlatform}
        onSuccess={() => fetch()}
      />
    </div>
  )
}

import { useEffect, useState } from 'react'
import { KtTabs, KtButton, KtConfirmDialog, KtAsyncState } from '@/shared/ui'
import { fetchPlugins, type PluginInfo } from '@/shared/services/plugin-service'
import { useAccountStore } from '../hooks/useAccountStore'
import type { AccountConfigUpdate } from '../services/account-service'
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
    error,
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
  const [deleteAccount, setDeleteAccount] = useState<Account | null>(null)
  const [pluginError, setPluginError] = useState('')

  useEffect(() => {
    fetchPlugins()
      .then((list) => {
        setPlugins(list)
        if (list.length > 0) {
          const first = list[0].platform_id
          if (!activePlatform) {
            setActivePlatform(first)
          } else {
            void fetch().catch(() => undefined)
          }
        }
      })
      .catch((error) => setPluginError(error instanceof Error ? error.message : '平台加载失败。'))
    // 页面加载时自动检查所有账号登录态
    void checkLogin().catch(() => undefined)
  }, [])

  const platformTabs = plugins.map((p) => ({ key: p.platform_id, label: p.display_name }))
  const activePlugin = plugins.find((p) => p.platform_id === activePlatform)

  const confirmDelete = async () => {
    if (!deleteAccount) return
    await remove(deleteAccount.id)
    setDeleteAccount(null)
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

  const handleSaveConfig = async (id: string, updates: AccountConfigUpdate) => {
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
          <KtButton
            variant="primary"
            size="sm"
            onClick={handleAddAccount}
            disabled={!activePlatform}
          >
            添加账号
          </KtButton>
        </div>
      </div>

      <div style={{ flex: 1, overflow: 'auto' }}>
        <KtAsyncState
          loading={loading}
          error={pluginError || error}
          empty={!loading && accounts.length === 0}
          emptyText="还没有账号。先选择平台并点击添加账号完成登录。"
          onRetry={() => fetch()}
        />
        {!loading && !pluginError && !error && accounts.length > 0 && (
          <AccountTable
            accounts={accounts}
            onConfigure={setConfigAccount}
            onRelogin={handleRelogin}
            onToggle={handleToggle}
            onDelete={setDeleteAccount}
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
      <KtConfirmDialog
        open={Boolean(deleteAccount)}
        title="删除账号"
        message={`确定删除「${deleteAccount?.name || deleteAccount?.email || '这个账号'}」吗？删除后正在使用这个账号的任务可能会失败。`}
        confirmText="删除"
        danger
        onCancel={() => setDeleteAccount(null)}
        onConfirm={confirmDelete}
      />
    </div>
  )
}

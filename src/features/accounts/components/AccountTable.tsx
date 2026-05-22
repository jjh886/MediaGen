import { KtTable, KtBadge, KtButton } from '@/shared/ui'
import type { Column } from '@/shared/ui'
import type { Account, LoginState } from '../types/account'

const stateVariant: Record<LoginState, 'success' | 'warning' | 'error' | 'disabled'> = {
  available: 'success',
  daily_quota_exhausted: 'warning',
  cooldown: 'warning',
  login_expired: 'error',
  total_quota_exhausted: 'error',
  verification_required: 'error',
  disabled: 'disabled',
}

const stateLabel: Record<LoginState, string> = {
  available: '可用',
  daily_quota_exhausted: '今日已满',
  cooldown: '冷却中',
  login_expired: '登录过期',
  total_quota_exhausted: '总量已满',
  verification_required: '需验证',
  disabled: '已停用',
}

interface AccountTableProps {
  accounts: Account[]
  onConfigure: (account: Account) => void
  onRelogin: (account: Account) => void
  onToggle: (account: Account) => void
  onDelete: (account: Account) => void
}

export default function AccountTable({
  accounts,
  onConfigure,
  onRelogin,
  onToggle,
  onDelete,
}: AccountTableProps) {
  const columns: Column<Account>[] = [
    {
      key: 'name',
      title: '名称',
      width: 180,
      render: (r) => (
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {r.avatar_url ? (
            <img
              src={r.avatar_url}
              alt=""
              style={{ width: 24, height: 24, borderRadius: '50%', objectFit: 'cover' }}
            />
          ) : (
            <div
              style={{
                width: 24,
                height: 24,
                borderRadius: '50%',
                background: 'var(--bg-tertiary)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 12,
                color: 'var(--text-secondary)',
              }}
            >
              {r.name?.[0] || '?'}
            </div>
          )}
          <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {r.name}
          </span>
        </div>
      ),
    },
    { key: 'email', title: '邮箱/手机', width: 160 },
    {
      key: 'tier',
      title: '等级',
      width: 80,
      render: (r) => <KtBadge variant="info">{r.tier}</KtBadge>,
    },
    {
      key: 'login_state',
      title: '状态',
      width: 90,
      render: (r) => (
        <KtBadge variant={stateVariant[r.login_state]}>{stateLabel[r.login_state]}</KtBadge>
      ),
    },
    {
      key: 'used_today',
      title: '今日用量',
      width: 100,
      render: (r) => `${r.used_today}/${r.daily_limit}`,
    },
    {
      key: 'used_total',
      title: '总用量',
      width: 100,
      render: (r) => `${r.used_total}/${r.total_limit}`,
    },
    {
      key: 'actions',
      title: '操作',
      width: 220,
      render: (r) => (
        <div style={{ display: 'flex', gap: 4 }}>
          <KtButton size="sm" variant="text" onClick={() => onConfigure(r)}>
            配置
          </KtButton>
          <KtButton size="sm" variant="text" onClick={() => onRelogin(r)}>
            重新登录
          </KtButton>
          <KtButton size="sm" variant="text" onClick={() => onToggle(r)}>
            {r.login_state === 'disabled' ? '启用' : '停用'}
          </KtButton>
          <KtButton
            size="sm"
            variant="text"
            style={{ color: 'var(--color-error)' }}
            onClick={() => onDelete(r)}
          >
            删除
          </KtButton>
        </div>
      ),
    },
  ]

  return <KtTable<Account> columns={columns} data={accounts} rowKey="id" />
}

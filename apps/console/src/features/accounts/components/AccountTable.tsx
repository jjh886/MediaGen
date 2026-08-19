import { KtTable, KtBadge, KtButton } from '@/shared/ui'
import type { Column } from '@/shared/ui'
import type { Account } from '../types/account'
import { accountResetText, accountStateLabel, accountStateVariant } from '../utils/account-status'

const browserOsLabel: Record<string, string> = {
  windows: 'Windows',
  macos: 'macOS',
  linux: 'Linux',
}

function AccountStateCell({ account }: { account: Account }) {
  const resetText = accountResetText(account)
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <KtBadge variant={accountStateVariant[account.login_state]}>
        {accountStateLabel[account.login_state]}
      </KtBadge>
      {resetText && (
        <span style={{ color: 'var(--text-disabled)', fontSize: 12 }}>{resetText}</span>
      )}
    </div>
  )
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
      width: 260,
      minWidth: 180,
      maxWidth: 420,
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
          <span
            style={{
              minWidth: 0,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
            title={r.name}
          >
            {r.name}
          </span>
        </div>
      ),
    },
    { key: 'email', title: '邮箱/手机', width: 260, minWidth: 180, maxWidth: 420 },
    {
      key: 'tier',
      title: '等级',
      width: 100,
      minWidth: 80,
      render: (r) => <KtBadge variant="info">{r.tier}</KtBadge>,
    },
    {
      key: 'login_state',
      title: '状态',
      width: 180,
      minWidth: 160,
      render: (r) => <AccountStateCell account={r} />,
    },
    {
      key: 'used_today',
      title: '今日用量',
      width: 120,
      minWidth: 100,
      render: (r) => `${r.used_today}/${r.daily_limit}`,
    },
    {
      key: 'used_total',
      title: '总用量',
      width: 120,
      minWidth: 100,
      render: (r) => `${r.used_total}/${r.total_limit}`,
    },
    {
      key: 'task_interval_seconds',
      title: '任务间隔',
      width: 120,
      minWidth: 100,
      render: (r) => (r.task_interval_seconds > 0 ? `${r.task_interval_seconds}秒` : '不等待'),
    },
    {
      key: 'browser_version',
      title: '浏览器环境',
      width: 150,
      minWidth: 130,
      render: (r) =>
        r.browser_os && r.browser_version
          ? `${browserOsLabel[r.browser_os] ?? r.browser_os} / Chrome ${r.browser_version}`
          : '默认',
    },
    {
      key: 'actions',
      title: '操作',
      width: 248,
      minWidth: 220,
      sticky: 'right',
      resizable: false,
      render: (r) => (
        <div style={{ display: 'flex', gap: 4, whiteSpace: 'nowrap' }}>
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

import { parseProxyUrlParts } from '@/shared/utils/proxy-url'
import type { ProxyItem } from '@/shared/services/proxy-service'
import { RefreshCw } from 'lucide-react'
import { KtTable, KtBadge, KtButton } from '@/shared/ui'
import type { Column } from '@/shared/ui'
import type { Account } from '../types/account'
import AccountQuotaSummary from './AccountQuotaSummary'
import { accountResetText, accountStateLabel, accountStateVariant } from '../utils/account-status'
import './account-table.css'

const browserOsLabel: Record<string, string> = {
  windows: 'Windows',
  macos: 'macOS',
  linux: 'Linux',
}

const actionStyle = { padding: '0 4px', justifyContent: 'flex-start' } as const

function RefreshQuotaIcon({
  label,
  busy,
  disabled,
  onClick,
}: {
  label: string
  busy: boolean
  disabled: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className="account-table__refresh"
      aria-label={label}
      title={busy ? '正在刷新额度…' : label}
      aria-busy={busy}
      disabled={disabled}
      onClick={onClick}
    >
      <RefreshCw
        size={14}
        aria-hidden="true"
        className={busy ? 'account-table__spinning' : undefined}
      />
    </button>
  )
}
function AccountIdentity({ account }: { account: Account }) {
  return (
    <div className="account-table__identity">
      {account.avatar_url ? (
        <img className="account-table__avatar" src={account.avatar_url} alt="" />
      ) : (
        <span className="account-table__avatar" aria-hidden="true">
          {account.name?.[0] || '?'}
        </span>
      )}
      <div className="account-table__identity-text">
        <div className="account-table__identity-name">
          <span className="account-table__ellipsis" title={account.name}>
            {account.name || '未命名账号'}
          </span>
          <KtBadge variant="info">{account.tier}</KtBadge>
        </div>
        <div className="account-table__contact account-table__ellipsis" title={account.email}>
          {account.email || '未提供邮箱/手机'}
        </div>
      </div>
    </div>
  )
}
function AccountStateCell({ account }: { account: Account }) {
  const resetText = accountResetText(account)
  return (
    <div
      className="account-table__state"
      title={resetText || accountStateLabel[account.login_state]}
    >
      <KtBadge variant={accountStateVariant[account.login_state]}>
        {accountStateLabel[account.login_state]}
      </KtBadge>
      {account.quota_task_kind && (
        <span>{account.quota_task_kind === 'text' ? '仅文本受限' : '仅媒体受限'}</span>
      )}
      {resetText && (
        <span className="account-table__reset">
          {resetText.replace('额度恢复时间 ', '')}
          <br />
          恢复
        </span>
      )}
    </div>
  )
}
function UsageCell({
  today,
  dailyLimit,
  total,
  totalLimit,
}: {
  today: number
  dailyLimit: number
  total: number
  totalLimit: number
}) {
  return (
    <div className="account-table__usage">
      <div>
        <span>今日</span>
        <span>
          {today}/{dailyLimit || '不限'}
        </span>
      </div>
      <div>
        <span>累计</span>
        <span>
          {total}/{totalLimit || '不限'}
        </span>
      </div>
    </div>
  )
}
interface AccountTableProps {
  accounts: Account[]
  proxies?: ProxyItem[]
  onRefreshQuota?: (account: Account) => void
  onRefreshAllQuota?: () => void
  quotaBusy?: string | null
  onConfigure: (account: Account) => void
  onRelogin: (account: Account) => void
  onToggle: (account: Account) => void
  onDelete: (account: Account) => void
}
export default function AccountTable({
  accounts,
  proxies = [],
  onConfigure,
  onRelogin,
  onToggle,
  onDelete,
  onRefreshQuota,
  onRefreshAllQuota,
  quotaBusy,
}: AccountTableProps) {
  const columns: Column<Account>[] = [
    {
      key: 'name',
      title: '账号',
      width: 200,
      minWidth: 180,
      maxWidth: 420,
      render: (r) => <AccountIdentity account={r} />,
    },
    {
      key: 'login_state',
      title: '状态',
      width: 86,
      minWidth: 86,
      maxWidth: 150,
      render: (r) => <AccountStateCell account={r} />,
    },
    {
      key: 'quota_windows',
      title: '剩余额度',
      width: 190,
      minWidth: 180,
      renderHeader: () => (
        <div className="account-table__quota-heading">
          {onRefreshAllQuota && (
            <RefreshQuotaIcon
              label="刷新当前列表账号额度"
              busy={quotaBusy === 'all'}
              disabled={!!quotaBusy || !accounts.some((a) => a.login_state !== 'disabled')}
              onClick={onRefreshAllQuota}
            />
          )}
          <span>剩余额度</span>
        </div>
      ),
      render: (r) => (
        <div className="account-table__quota">
          {onRefreshQuota && (
            <RefreshQuotaIcon
              label={`刷新额度 ${r.name}`}
              busy={quotaBusy === r.id || (quotaBusy === 'all' && r.login_state !== 'disabled')}
              disabled={!!quotaBusy}
              onClick={() => onRefreshQuota(r)}
            />
          )}
          <div className="account-table__quota-content">
            <AccountQuotaSummary account={r} compact />
          </div>
        </div>
      ),
    },
    {
      key: 'media_usage',
      title: '媒体用量',
      width: 100,
      minWidth: 100,
      render: (r) => (
        <UsageCell
          today={r.used_today}
          dailyLimit={r.daily_limit}
          total={r.used_total}
          totalLimit={r.total_limit}
        />
      ),
    },
    {
      key: 'text_usage',
      title: '文本用量',
      width: 100,
      minWidth: 100,
      render: (r) => (
        <UsageCell
          today={r.text_used_today ?? 0}
          dailyLimit={r.text_daily_limit ?? 0}
          total={r.text_used_total ?? 0}
          totalLimit={r.text_total_limit ?? 0}
        />
      ),
    },
    {
      key: 'task_interval_seconds',
      title: '任务间隔',
      width: 76,
      minWidth: 76,
      render: (r) => (
        <span className="account-table__small">
          {r.task_interval_seconds > 0 ? `${r.task_interval_seconds}秒` : '不等待'}
        </span>
      ),
    },
    {
      key: 'browser_version',
      title: '浏览器环境',
      width: 100,
      minWidth: 100,
      render: (r) =>
        r.browser_os && r.browser_version ? (
          <div className="account-table__browser">
            <span>{browserOsLabel[r.browser_os] ?? r.browser_os}</span>
            <span>Chrome {r.browser_version}</span>
          </div>
        ) : (
          <span className="account-table__small">默认</span>
        ),
    },
    {
      key: 'proxy',
      title: '代理',
      width: 160,
      minWidth: 120,
      maxWidth: 320,
      render: (r) => {
        const proxy = proxies.find((item) => item.id === r.proxy_id)
        const parts = parseProxyUrlParts(r.proxy_id ? proxy?.server || '' : r.proxy_url || '')
        const address = parts
          ? `${parts.protocol}://${parts.host}${parts.port ? `:${parts.port}` : ''}`
          : ''
        const label = r.proxy_id
          ? proxy?.name || address || `代理 ${r.proxy_id}`
          : address || (r.proxy_url ? '独立代理' : '未设置')
        return (
          <div className="account-table__small account-table__ellipsis" title={address || label}>
            <span>{label}</span>
            {proxy && !proxy.enabled && <span>（已停用）</span>}
          </div>
        )
      },
    },
    {
      key: 'actions',
      title: '操作',
      width: 136,
      minWidth: 136,
      sticky: 'right',
      resizable: false,
      render: (r) => (
        <div className="account-table__actions">
          <KtButton size="sm" variant="text" style={actionStyle} onClick={() => onConfigure(r)}>
            配置
          </KtButton>
          <KtButton size="sm" variant="text" style={actionStyle} onClick={() => onRelogin(r)}>
            重新登录
          </KtButton>
          <KtButton size="sm" variant="text" style={actionStyle} onClick={() => onToggle(r)}>
            {r.login_state === 'disabled' ? '启用' : '停用'}
          </KtButton>
          <KtButton
            size="sm"
            variant="text"
            style={{ ...actionStyle, color: 'var(--color-error)' }}
            onClick={() => onDelete(r)}
          >
            删除
          </KtButton>
        </div>
      ),
    },
  ]
  return (
    <div className="account-table">
      <KtTable<Account> columns={columns} data={accounts} rowKey="id" />
    </div>
  )
}

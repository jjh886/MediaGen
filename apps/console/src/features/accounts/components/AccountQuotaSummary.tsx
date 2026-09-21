import type { Account } from '../types/account'

export default function AccountQuotaSummary({ account }: { account: Account }) {
  const windows = account.quota_windows || []
  const holds = Object.entries(account.quota_holds || {}).filter(
    ([, hold]) => !hold.until || Date.parse(hold.until) > Date.now()
  )
  if (!windows.length && !holds.length)
    return <span style={{ color: 'var(--text-disabled)' }}>平台未提供额度数据</span>
  return (
    <div style={{ display: 'grid', gap: 10, minWidth: 170 }}>
      {holds.map(([kind, hold]) => (
        <div key={kind} style={{ fontSize: 11, color: 'var(--color-warning)' }}>
          {kind === 'text' ? '文本' : '图片 / 媒体'}额度已用完
          <div>
            {hold.until ? `恢复：${new Date(hold.until).toLocaleString()}` : '等待平台额度恢复'}
          </div>
        </div>
      ))}
      {windows.map((window, index) => (
        <div key={`${window.name}-${index}`}>
          <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, fontSize: 12 }}>
            <span>{window.name}</span>
            <strong>
              {typeof window.remaining_percent === 'number'
                ? `${Math.round(window.remaining_percent * 10) / 10}% 剩余`
                : typeof window.remaining_count === 'number'
                  ? `剩余 ${window.remaining_count} 次`
                  : '未知'}
            </strong>
          </div>
          {typeof window.remaining_percent === 'number' && (
            <progress
              aria-label={`${window.name}剩余额度`}
              max={100}
              value={window.remaining_percent}
              style={{
                width: '100%',
                height: 5,
                accentColor: window.remaining_percent > 20 ? 'var(--color-brand)' : '#ed9a50',
              }}
            />
          )}
          {window.reset_at && (
            <div style={{ fontSize: 11, color: 'var(--text-secondary)' }}>
              恢复：{new Date(window.reset_at).toLocaleString()}
            </div>
          )}
        </div>
      ))}
    </div>
  )
}

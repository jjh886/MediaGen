import { KtButton } from './KtButton'

interface KtAsyncStateProps {
  loading?: boolean
  error?: string
  empty?: boolean
  emptyText?: string
  onRetry?: () => void
}

const stateStyle: React.CSSProperties = {
  padding: 32,
  color: 'var(--text-secondary)',
  textAlign: 'center',
}

export function KtAsyncState({
  loading,
  error,
  empty,
  emptyText = '暂无数据',
  onRetry,
}: KtAsyncStateProps) {
  if (!loading && !error && !empty) return null

  return (
    <div style={stateStyle}>
      <div>{loading ? '加载中...' : error || emptyText}</div>
      {error && onRetry && (
        <KtButton size="sm" style={{ marginTop: 12 }} onClick={onRetry}>
          重试
        </KtButton>
      )}
    </div>
  )
}

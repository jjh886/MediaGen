import { useEffect, useRef } from 'react'

interface InfiniteScrollTriggerProps {
  hasMore: boolean
  loading: boolean
  onLoadMore: () => void
}

/** 提前 240px 请求下一页，让用户滚动时几乎感觉不到分页。 */
export function InfiniteScrollTrigger({
  hasMore,
  loading,
  onLoadMore,
}: InfiniteScrollTriggerProps) {
  const triggerRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const trigger = triggerRef.current
    if (!trigger || !hasMore || loading) return
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting) onLoadMore()
      },
      { rootMargin: '240px 0px' }
    )
    observer.observe(trigger)
    return () => observer.disconnect()
  }, [hasMore, loading, onLoadMore])

  return (
    <div
      ref={triggerRef}
      aria-live="polite"
      style={{ minHeight: 1, padding: loading ? '12px 0' : 0, textAlign: 'center' }}
    >
      {loading ? (
        <span style={{ color: 'var(--text-secondary)', fontSize: 12 }}>加载中...</span>
      ) : null}
    </div>
  )
}

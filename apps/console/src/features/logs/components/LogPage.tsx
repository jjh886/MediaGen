import { useEffect, useCallback, useRef, type KeyboardEvent, type RefObject } from 'react'
import { KtAsyncState, KtSelect, KtInput } from '@/shared/ui'
import { useLogStore, type LogState } from '../hooks/useLogStore'
import { LogTable } from './LogTable'

const MODULE_OPTIONS = [
  { value: '', label: '全部模块' },
  { value: 'engine', label: '引擎' },
  { value: 'scheduler', label: '调度器' },
  { value: 'account', label: '账号' },
  { value: 'api', label: 'API' },
  { value: 'registration', label: '注册' },
]

const LEVEL_OPTIONS = [
  { value: '', label: '全部级别' },
  { value: 'debug', label: 'Debug' },
  { value: 'info', label: 'Info' },
  { value: 'warn', label: 'Warn' },
  { value: 'error', label: 'Error' },
]

export function LogPage({ initialTaskId }: { initialTaskId?: string | null }) {
  const store = useLogStore()
  const load = store.load
  const refreshLatest = store.refreshLatest
  const setTaskId = store.setTaskId
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (initialTaskId) setTaskId(initialTaskId)
    load()
    const timer = setInterval(() => {
      refreshLatest()
    }, 3000)
    return () => clearInterval(timer)
  }, [initialTaskId, load, refreshLatest, setTaskId])

  const handleSearch = useCallback(() => {
    scrollRef.current?.scrollTo({ top: 0 })
    load()
  }, [load])

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === 'Enter') handleSearch()
    },
    [handleSearch]
  )

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16, height: '100%' }}>
      <LogFilters
        store={store}
        onKeyDown={handleKeyDown}
        onReset={() => scrollRef.current?.scrollTo({ top: 0 })}
      />
      <LogContent store={store} scrollRef={scrollRef} />
    </div>
  )
}

function LogFilters({
  store,
  onKeyDown,
  onReset,
}: {
  store: LogState
  onKeyDown: (event: KeyboardEvent) => void
  onReset: () => void
}) {
  return (
    <div style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
      <KtSelect
        value={store.moduleFilter}
        onChange={(value) => {
          onReset()
          store.setModuleFilter(value)
        }}
        options={MODULE_OPTIONS}
      />
      <KtSelect
        value={store.levelFilter}
        onChange={(value) => {
          onReset()
          store.setLevelFilter(value)
        }}
        options={LEVEL_OPTIONS}
      />
      <KtInput
        placeholder="搜索日志..."
        value={store.keyword}
        onChange={(e) => store.setKeyword(e.target.value)}
        onKeyDown={onKeyDown}
        style={{ maxWidth: 260 }}
      />
      <KtInput
        placeholder="任务 ID"
        value={store.taskId}
        onChange={(e) => store.setTaskId(e.target.value)}
        onKeyDown={onKeyDown}
        style={{ maxWidth: 220 }}
      />
    </div>
  )
}

function LogContent({
  store,
  scrollRef,
}: {
  store: LogState
  scrollRef: RefObject<HTMLDivElement | null>
}) {
  return (
    <div
      ref={scrollRef}
      style={{ flex: 1, overflow: 'auto', borderRadius: 12, background: '#1E1F21' }}
    >
      {store.error ? (
        <KtAsyncState error={store.error} onRetry={store.load} />
      ) : (
        <LogTable
          logs={store.logs}
          loading={store.loading}
          expandedId={store.expandedId}
          onToggle={store.toggleExpand}
          hasMore={store.hasMore}
          loadingMore={store.loadingMore}
          onLoadMore={() => void store.loadMore()}
        />
      )}
    </div>
  )
}

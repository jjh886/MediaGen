import { useEffect, useCallback, type KeyboardEvent } from 'react'
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
  const setTaskId = store.setTaskId

  useEffect(() => {
    if (initialTaskId) setTaskId(initialTaskId)
    load()
    const timer = setInterval(() => {
      load()
    }, 3000)
    return () => clearInterval(timer)
  }, [initialTaskId, load, setTaskId])

  const handleSearch = useCallback(() => {
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
      <LogFilters store={store} onKeyDown={handleKeyDown} />
      <LogContent store={store} />
    </div>
  )
}

function LogFilters({
  store,
  onKeyDown,
}: {
  store: LogState
  onKeyDown: (event: KeyboardEvent) => void
}) {
  return (
    <div style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
      <KtSelect
        value={store.moduleFilter}
        onChange={store.setModuleFilter}
        options={MODULE_OPTIONS}
      />
      <KtSelect value={store.levelFilter} onChange={store.setLevelFilter} options={LEVEL_OPTIONS} />
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

function LogContent({ store }: { store: LogState }) {
  return (
    <div style={{ flex: 1, overflow: 'auto', borderRadius: 12, background: '#1E1F21' }}>
      {store.error ? (
        <KtAsyncState error={store.error} onRetry={store.load} />
      ) : (
        <LogTable
          logs={store.logs}
          loading={store.loading}
          expandedId={store.expandedId}
          onToggle={store.toggleExpand}
        />
      )}
    </div>
  )
}

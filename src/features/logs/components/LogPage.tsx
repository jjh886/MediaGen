import { useEffect, useCallback } from 'react'
import { KtSelect, KtInput } from '@/shared/ui'
import { useLogStore } from '../hooks/useLogStore'
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

export function LogPage() {
  const {
    logs,
    loading,
    moduleFilter,
    levelFilter,
    keyword,
    setModuleFilter,
    setLevelFilter,
    setKeyword,
    expandedId,
    toggleExpand,
    load,
  } = useLogStore()

  useEffect(() => {
    load()
    const timer = setInterval(() => {
      load()
    }, 3000)
    return () => clearInterval(timer)
  }, [load])

  const handleSearch = useCallback(() => {
    load()
  }, [load])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') handleSearch()
    },
    [handleSearch]
  )

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16, height: '100%' }}>
      <div style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
        <KtSelect value={moduleFilter} onChange={setModuleFilter} options={MODULE_OPTIONS} />
        <KtSelect value={levelFilter} onChange={setLevelFilter} options={LEVEL_OPTIONS} />
        <KtInput
          placeholder="搜索日志..."
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          onKeyDown={handleKeyDown}
          style={{ maxWidth: 260 }}
        />
      </div>

      <div style={{ flex: 1, overflow: 'auto', borderRadius: 12, background: '#1E1F21' }}>
        <LogTable logs={logs} loading={loading} expandedId={expandedId} onToggle={toggleExpand} />
      </div>
    </div>
  )
}

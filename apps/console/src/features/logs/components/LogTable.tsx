import { KtBadge, KtTable } from '@/shared/ui'
import type { Column } from '@/shared/ui'
import type { LogEntry } from '../services/log-service'
import { LogDetailRow } from './LogDetailRow'

const LEVEL_VARIANT: Record<string, 'disabled' | 'info' | 'warning' | 'error'> = {
  debug: 'disabled',
  info: 'info',
  warn: 'warning',
  error: 'error',
}

const columns: Column<LogEntry>[] = [
  { key: 'timestamp', title: '时间', width: 230, minWidth: 180, maxWidth: 320 },
  { key: 'module', title: '模块', width: 140, minWidth: 100, maxWidth: 260 },
  {
    key: 'level',
    title: '级别',
    width: 110,
    minWidth: 90,
    render: (r) => <KtBadge variant={LEVEL_VARIANT[r.level]}>{r.level}</KtBadge>,
  },
  {
    key: 'task_id',
    title: '任务',
    width: 140,
    minWidth: 100,
    render: (r) => r.task_id?.slice(0, 8) || '-',
  },
  { key: 'summary', title: '摘要', width: 520, minWidth: 240, maxWidth: 900 },
]

interface LogTableProps {
  logs: LogEntry[]
  loading: boolean
  expandedId: string | null
  onToggle: (id: string) => void
}

export function LogTable({ logs, loading, expandedId, onToggle }: LogTableProps) {
  if (loading) {
    return (
      <div style={{ padding: 32, textAlign: 'center', color: 'rgba(255,255,255,0.4)' }}>
        加载中...
      </div>
    )
  }

  return (
    <KtTable<LogEntry>
      columns={columns}
      data={logs}
      rowKey="id"
      selectedRowKey={expandedId ?? undefined}
      emptyText="暂无日志"
      onRowClick={(log) => onToggle(log.id)}
      isRowExpanded={(log) => expandedId === log.id}
      expandedRowRender={(log) => <LogDetailRow detail={log.detail} />}
    />
  )
}

import { KtBadge } from '@/shared/ui'
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
  { key: 'timestamp', title: '时间', width: 180 },
  { key: 'module', title: '模块', width: 100 },
  {
    key: 'level',
    title: '级别',
    width: 90,
    render: (r) => <KtBadge variant={LEVEL_VARIANT[r.level]}>{r.level}</KtBadge>,
  },
  { key: 'summary', title: '摘要' },
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
    <table style={{ width: '100%', borderCollapse: 'collapse', tableLayout: 'fixed' }}>
      <thead>
        <tr>
          {columns.map((col) => (
            <th
              key={col.key}
              style={{
                background: 'rgba(31,31,31,0.5)',
                color: 'rgba(255,255,255,0.6)',
                fontSize: 13,
                fontWeight: 500,
                padding: '0 16px',
                height: 40,
                textAlign: 'left',
                borderBottom: '1px solid rgba(255,255,255,0.05)',
                width: col.width,
              }}
            >
              {col.title}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {logs.length === 0 ? (
          <tr>
            <td
              colSpan={4}
              style={{ textAlign: 'center', color: 'rgba(255,255,255,0.4)', padding: 32 }}
            >
              暂无日志
            </td>
          </tr>
        ) : (
          logs.map((log) => (
            <LogRow
              key={log.id}
              log={log}
              expanded={expandedId === log.id}
              onToggle={() => onToggle(log.id)}
            />
          ))
        )}
      </tbody>
    </table>
  )
}

function LogRow({
  log,
  expanded,
  onToggle,
}: {
  log: LogEntry
  expanded: boolean
  onToggle: () => void
}) {
  return (
    <>
      <tr
        onClick={onToggle}
        style={{
          height: 48,
          borderBottom: '1px solid rgba(255,255,255,0.05)',
          cursor: 'pointer',
          background: expanded ? 'rgba(90,176,254,0.05)' : 'transparent',
        }}
      >
        {columns.map((col) => (
          <td
            key={col.key}
            style={{
              padding: '0 16px',
              fontSize: 14,
              color: 'rgba(255,255,255,0.85)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              width: col.width,
            }}
          >
            {col.render ? col.render(log, 0) : String(log[col.key as keyof LogEntry] ?? '')}
          </td>
        ))}
      </tr>
      {expanded && <LogDetailRow detail={log.detail} />}
    </>
  )
}

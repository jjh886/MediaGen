import { useEffect, useMemo, useState } from 'react'
import { KtTable, KtBadge, KtTabs, KtButton, KtConfirmDialog, KtAsyncState } from '@/shared/ui'
import type { Column } from '@/shared/ui'
import { fetchPlugins, type PluginInfo } from '@/shared/services/plugin-service'
import { fetchAccounts } from '@/shared/services/account-service'
import type { Account } from '@/shared/types/account'
import type { Task, TaskKind } from '../types/task'
import { statusVariant, statusLabel } from '../types/task'
import { useTaskStore } from '../hooks/useTaskStore'
import TaskDetail from './TaskDetail'

const kindLabel: Record<TaskKind, string> = {
  image: '图片',
  video: '视频',
}

function formatDuration(task: Task): string {
  if (!task.completed_at) return '-'
  const ms = new Date(task.completed_at).getTime() - new Date(task.created_at).getTime()
  const sec = Math.round(ms / 1000)
  return sec >= 60 ? `${Math.floor(sec / 60)}分${sec % 60}秒` : `${sec}秒`
}

function formatTime(iso: string): string {
  const d = new Date(iso)
  return `${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

function cancelErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message
  if (typeof error === 'string' && error.trim()) return error
  return '取消任务失败，请稍后重试'
}

export default function TaskList({
  onOpenVerification,
  onOpenTaskLogs,
}: {
  onOpenVerification?: (taskId: string) => void
  onOpenTaskLogs?: (taskId: string) => void
}) {
  const {
    tasks,
    selectedTaskId,
    activePlatform,
    loading,
    error,
    fetch,
    setActivePlatform,
    selectTask,
    cancel,
  } = useTaskStore()
  const selectedTask = tasks.find((t) => t.id === selectedTaskId) ?? null

  const [plugins, setPlugins] = useState<PluginInfo[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])
  const [cancelTarget, setCancelTarget] = useState<Task | null>(null)
  const [cancelingTaskId, setCancelingTaskId] = useState<string | null>(null)
  const [cancelError, setCancelError] = useState('')
  const [pluginError, setPluginError] = useState('')

  const accountMap = useMemo(
    () => new Map(accounts.map((a) => [a.id, a.name || a.email])),
    [accounts]
  )

  useEffect(() => {
    fetchPlugins()
      .then((list) => {
        setPlugins(list)
        if (list.length > 0 && !activePlatform) {
          setActivePlatform(list[0].platform_id)
        } else {
          void fetch().catch(() => undefined)
        }
      })
      .catch((error) => setPluginError(error instanceof Error ? error.message : '平台加载失败。'))
  }, [])

  useEffect(() => {
    if (activePlatform) {
      fetchAccounts(activePlatform)
        .then(setAccounts)
        .catch(() => setAccounts([]))
    }
  }, [activePlatform])

  const platformTabs = plugins.map((p) => ({ key: p.platform_id, label: p.display_name }))

  const confirmCancel = async () => {
    if (!cancelTarget || cancelingTaskId) return
    setCancelError('')
    setCancelingTaskId(cancelTarget.id)
    try {
      await cancel(cancelTarget.id)
      setCancelTarget(null)
    } catch (error) {
      setCancelError(cancelErrorMessage(error))
    } finally {
      setCancelingTaskId(null)
    }
  }

  const columns: Column<Task>[] = [
    {
      key: 'id',
      title: 'ID',
      width: 96,
      minWidth: 80,
      render: (r) => (
        <span style={{ fontFamily: 'monospace', fontSize: 12 }}>{r.id.slice(0, 8)}</span>
      ),
    },
    {
      key: 'task_kind',
      title: '类型',
      width: 90,
      minWidth: 72,
      render: (r) => <KtBadge variant="info">{kindLabel[r.task_kind]}</KtBadge>,
    },
    { key: 'model_id', title: '模型', width: 180, minWidth: 120, maxWidth: 320 },
    {
      key: 'prompt',
      title: '提示词',
      width: 420,
      minWidth: 220,
      maxWidth: 900,
      render: (r) => (
        <span
          style={{
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            display: 'block',
            width: '100%',
          }}
          title={r.prompt}
        >
          {r.prompt}
        </span>
      ),
    },
    {
      key: 'account_id',
      title: '账号',
      width: 160,
      minWidth: 120,
      maxWidth: 360,
      render: (r) => {
        if (!r.account_id) return <span style={{ color: 'var(--text-tertiary)' }}>-</span>
        const name = accountMap.get(r.account_id)
        return (
          <span
            style={{
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              display: 'block',
              width: '100%',
            }}
            title={name ?? r.account_id}
          >
            {name ?? r.account_id.slice(0, 8)}
          </span>
        )
      },
    },
    {
      key: 'status',
      title: '状态',
      width: 100,
      minWidth: 90,
      render: (r) => <KtBadge variant={statusVariant[r.status]}>{statusLabel[r.status]}</KtBadge>,
    },
    {
      key: 'duration',
      title: '耗时',
      width: 110,
      minWidth: 90,
      render: (r) => formatDuration(r),
    },
    {
      key: 'created_at',
      title: '时间',
      width: 140,
      minWidth: 120,
      render: (r) => formatTime(r.created_at),
    },
    {
      key: 'actions',
      title: '',
      width: 90,
      minWidth: 70,
      render: (r) =>
        r.status === 'pending' || r.status === 'queued' || r.status === 'processing' ? (
          <KtButton
            variant="default"
            size="sm"
            loading={cancelingTaskId === r.id}
            onClick={(e) => {
              e.stopPropagation()
              setCancelTarget(r)
            }}
          >
            取消
          </KtButton>
        ) : r.status === 'verification_required' ? (
          <KtButton
            variant="primary"
            size="sm"
            onClick={(e) => {
              e.stopPropagation()
              onOpenVerification?.(r.id)
            }}
          >
            去处理
          </KtButton>
        ) : null,
    },
  ]

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: 16 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <KtTabs items={platformTabs} activeKey={activePlatform} onChange={setActivePlatform} />
        <KtButton variant="default" size="sm" onClick={() => fetch()}>
          刷新
        </KtButton>
      </div>

      <div style={{ flex: 1, overflow: 'auto' }}>
        {cancelError && (
          <div
            role="alert"
            style={{ color: 'var(--color-error)', paddingBottom: 12, fontSize: 13 }}
          >
            {cancelError}
          </div>
        )}
        <KtAsyncState
          loading={loading}
          error={pluginError || error}
          empty={!loading && tasks.length === 0}
          emptyText="暂无任务记录"
          onRetry={() => fetch()}
        />
        {!loading && !pluginError && !error && tasks.length > 0 && (
          <KtTable<Task>
            columns={columns}
            data={tasks}
            rowKey="id"
            selectedRowKey={selectedTaskId ?? undefined}
            onRowClick={(r) => selectTask(r.id === selectedTaskId ? null : r.id)}
          />
        )}
      </div>

      {selectedTask && (
        <TaskDetail
          task={selectedTask}
          accountLabel={accountMap.get(selectedTask.account_id ?? '')}
          onCancel={setCancelTarget}
          onOpenVerification={onOpenVerification}
          onOpenTaskLogs={onOpenTaskLogs}
          onClose={() => selectTask(null)}
          cancelling={cancelingTaskId === selectedTask.id}
        />
      )}
      <KtConfirmDialog
        open={Boolean(cancelTarget)}
        title="取消任务"
        message={`确定取消任务 ${cancelTarget?.id.slice(0, 8) || ''} 吗？取消后任务状态会变成已取消。`}
        confirmText="取消任务"
        danger
        loading={Boolean(cancelingTaskId)}
        onCancel={() => {
          if (!cancelingTaskId) setCancelTarget(null)
        }}
        onConfirm={() => void confirmCancel()}
      />
    </div>
  )
}

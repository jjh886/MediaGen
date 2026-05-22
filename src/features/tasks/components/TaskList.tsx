import { useEffect, useMemo, useState } from 'react'
import { KtTable, KtBadge, KtTabs, KtButton } from '@/shared/ui'
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

export default function TaskList() {
  const {
    tasks,
    selectedTaskId,
    activePlatform,
    loading,
    fetch,
    setActivePlatform,
    selectTask,
    cancel,
  } = useTaskStore()
  const selectedTask = tasks.find((t) => t.id === selectedTaskId) ?? null

  const [plugins, setPlugins] = useState<PluginInfo[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])

  const accountMap = useMemo(
    () => new Map(accounts.map((a) => [a.id, a.name || a.email])),
    [accounts]
  )

  useEffect(() => {
    fetchPlugins().then((list) => {
      setPlugins(list)
      if (list.length > 0 && !activePlatform) {
        setActivePlatform(list[0].platform_id)
      } else {
        fetch()
      }
    })
  }, [])

  useEffect(() => {
    if (activePlatform) {
      fetchAccounts(activePlatform).then(setAccounts)
    }
  }, [activePlatform])

  const platformTabs = plugins.map((p) => ({ key: p.platform_id, label: p.display_name }))

  const handleCancel = async (task: Task) => {
    await cancel(task.id)
  }

  const columns: Column<Task>[] = [
    {
      key: 'id',
      title: 'ID',
      width: 80,
      render: (r) => (
        <span style={{ fontFamily: 'monospace', fontSize: 12 }}>{r.id.slice(0, 8)}</span>
      ),
    },
    {
      key: 'task_kind',
      title: '类型',
      width: 60,
      render: (r) => <KtBadge variant="info">{kindLabel[r.task_kind]}</KtBadge>,
    },
    { key: 'model_id', title: '模型', width: 140 },
    {
      key: 'prompt',
      title: '提示词',
      render: (r) => (
        <span
          style={{
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            display: 'block',
            maxWidth: 200,
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
      width: 100,
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
              maxWidth: 100,
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
      width: 80,
      render: (r) => <KtBadge variant={statusVariant[r.status]}>{statusLabel[r.status]}</KtBadge>,
    },
    {
      key: 'duration',
      title: '耗时',
      width: 80,
      render: (r) => formatDuration(r),
    },
    {
      key: 'created_at',
      title: '时间',
      width: 110,
      render: (r) => formatTime(r.created_at),
    },
    {
      key: 'actions',
      title: '',
      width: 70,
      render: (r) =>
        r.status === 'pending' || r.status === 'processing' ? (
          <KtButton
            variant="default"
            size="sm"
            onClick={(e) => {
              e.stopPropagation()
              handleCancel(r)
            }}
          >
            取消
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
        {loading ? (
          <div style={{ color: 'var(--text-secondary)', padding: 32, textAlign: 'center' }}>
            加载中...
          </div>
        ) : tasks.length === 0 ? (
          <div style={{ color: 'var(--text-secondary)', padding: 32, textAlign: 'center' }}>
            暂无任务记录
          </div>
        ) : (
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
          onCancel={handleCancel}
          onClose={() => selectTask(null)}
        />
      )}
    </div>
  )
}

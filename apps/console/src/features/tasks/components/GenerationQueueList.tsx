import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ArrowUp, RefreshCw, X } from 'lucide-react'
import { InfiniteScrollTrigger, KtBadge, KtButton, KtModal, KtTable } from '@/shared/ui'
import type { Column } from '@/shared/ui'
import { useGenerationQueueStore } from '../hooks/useGenerationQueueStore'
import type { GenerationQueueItem } from '../types/generation-queue'

const POLL_INTERVAL_MS = 3000

function formatTime(value: string): string {
  return new Date(value).toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export default function GenerationQueueList() {
  const {
    items,
    total,
    loading,
    loadingMore,
    hasMore,
    error,
    load,
    loadMore,
    refreshLatest,
    runAction,
  } = useGenerationQueueStore()
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [cancelTaskIds, setCancelTaskIds] = useState<string[]>([])
  const [actionRunning, setActionRunning] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)
  const refresh = useCallback(() => {
    scrollRef.current?.scrollTo({ top: 0 })
    return load()
  }, [load])

  useEffect(() => {
    void load()
    const timer = window.setInterval(() => void refreshLatest(), POLL_INTERVAL_MS)
    return () => window.clearInterval(timer)
  }, [load, refreshLatest])

  useEffect(() => {
    const available = new Set(items.map((item) => item.taskId))
    setSelected((current) => new Set([...current].filter((id) => available.has(id))))
  }, [items])

  const toggle = (taskId: string) => {
    setSelected((current) => {
      const next = new Set(current)
      if (next.has(taskId)) next.delete(taskId)
      else next.add(taskId)
      return next
    })
  }

  const executeAction = async (action: 'promote' | 'cancel', taskIds: string[]) => {
    setActionRunning(true)
    try {
      await runAction(action, taskIds)
      setSelected(new Set())
    } catch {
      // Store 已保存可读的错误信息，由页面顶部统一显示。
    } finally {
      setActionRunning(false)
    }
  }

  const act = (action: 'promote' | 'cancel', taskIds: string[]) => {
    if (action === 'cancel') {
      setCancelTaskIds(taskIds)
      return
    }
    void executeAction(action, taskIds)
  }

  const confirmCancel = async () => {
    const taskIds = cancelTaskIds
    await executeAction('cancel', taskIds)
    setCancelTaskIds([])
  }

  const columns = useMemo<Column<GenerationQueueItem>[]>(
    () => [
      {
        key: 'selected',
        title: '',
        width: 48,
        minWidth: 48,
        maxWidth: 48,
        resizable: false,
        render: (item) => (
          <input
            type="checkbox"
            aria-label={`选择任务 ${item.taskId}`}
            checked={selected.has(item.taskId)}
            onChange={() => toggle(item.taskId)}
            onClick={(event) => event.stopPropagation()}
          />
        ),
      },
      { key: 'position', title: '顺序', width: 72, minWidth: 64, maxWidth: 88 },
      { key: 'platform', title: '平台', width: 120, minWidth: 100, maxWidth: 180 },
      { key: 'modelId', title: '模型', width: 180, minWidth: 130, maxWidth: 300 },
      {
        key: 'prompt',
        title: '提示词',
        width: 480,
        minWidth: 240,
        maxWidth: 900,
        render: (item) => (
          <span
            title={item.prompt}
            style={{ display: 'block', overflow: 'hidden', textOverflow: 'ellipsis' }}
          >
            {item.prompt}
          </span>
        ),
      },
      {
        key: 'status',
        title: '状态',
        width: 100,
        minWidth: 90,
        render: () => <KtBadge variant="info">排队中</KtBadge>,
      },
      {
        key: 'queuedAt',
        title: '排队时间',
        width: 150,
        minWidth: 130,
        render: (item) => formatTime(item.queuedAt),
      },
      {
        key: 'actions',
        title: '',
        width: 160,
        minWidth: 150,
        resizable: false,
        sticky: 'right',
        render: (item) => (
          <div style={{ display: 'flex', gap: 6 }}>
            <KtButton
              size="sm"
              title="插队"
              disabled={actionRunning}
              onClick={() => act('promote', [item.taskId])}
            >
              <ArrowUp size={14} />
              插队
            </KtButton>
            <KtButton
              size="sm"
              variant="danger"
              title="取消"
              disabled={actionRunning}
              onClick={() => act('cancel', [item.taskId])}
            >
              <X size={14} />
              取消
            </KtButton>
          </div>
        ),
      },
    ],
    [selected, actionRunning]
  )

  const selectedIds = [...selected]

  return (
    <div style={{ display: 'flex', minHeight: 0, flex: 1, flexDirection: 'column', gap: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <KtButton
            size="sm"
            onClick={() => setSelected(new Set(items.map((item) => item.taskId)))}
            disabled={items.length === 0}
          >
            全选
          </KtButton>
          <KtButton size="sm" onClick={() => setSelected(new Set())} disabled={selected.size === 0}>
            清空
          </KtButton>
          <span style={{ color: 'var(--text-secondary)', fontSize: 12 }}>
            {selected.size > 0 ? `已选 ${selected.size} 项` : `共 ${total} 项`}
          </span>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <KtButton
            size="sm"
            onClick={() => act('promote', selectedIds)}
            disabled={selected.size === 0 || actionRunning}
          >
            <ArrowUp size={14} />
            批量插队
          </KtButton>
          <KtButton
            size="sm"
            variant="danger"
            onClick={() => act('cancel', selectedIds)}
            disabled={selected.size === 0 || actionRunning}
          >
            <X size={14} />
            批量取消
          </KtButton>
          <KtButton size="sm" title="刷新" onClick={() => void refresh()} loading={loading}>
            <RefreshCw size={14} />
            刷新
          </KtButton>
        </div>
      </div>

      {error && (
        <div role="alert" style={{ color: 'var(--color-error)', fontSize: 12 }}>
          {error}
        </div>
      )}
      <div ref={scrollRef} style={{ minHeight: 0, flex: 1, overflow: 'auto' }}>
        <KtTable columns={columns} data={items} rowKey="taskId" emptyText="暂无排队任务" />
        <InfiniteScrollTrigger
          hasMore={!loading && hasMore}
          loading={loadingMore}
          onLoadMore={() => void loadMore().catch(() => undefined)}
        />
      </div>

      <KtModal
        open={cancelTaskIds.length > 0}
        title="取消排队任务"
        width={400}
        closeOnOverlayClick={!actionRunning}
        onClose={() => {
          if (!actionRunning) setCancelTaskIds([])
        }}
        footer={
          <>
            <KtButton disabled={actionRunning} onClick={() => setCancelTaskIds([])}>
              返回
            </KtButton>
            <KtButton variant="danger" loading={actionRunning} onClick={() => void confirmCancel()}>
              确认取消
            </KtButton>
          </>
        }
      >
        <div style={{ color: 'var(--text-primary)', lineHeight: 1.6 }}>
          确定取消 {cancelTaskIds.length} 个排队任务吗？
        </div>
      </KtModal>
    </div>
  )
}

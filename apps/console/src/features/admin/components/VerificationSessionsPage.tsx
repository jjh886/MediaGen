import { useCallback, useEffect, useState, type CSSProperties } from 'react'
import { RotateCw } from 'lucide-react'
import BrowserHistoryControls from '@/shared/browser/BrowserHistoryControls'
import RemoteBrowserPanel from '@/shared/browser/RemoteBrowserPanel'
import {
  browserSessionHistory,
  cancelBrowserSession,
  completeBrowserSession,
  fetchBrowserSession,
  type BrowserHistoryAction,
  type BrowserSession,
} from '@/shared/services/browser-session-service'
import { KtAsyncState, KtButton, KtConfirmDialog, KtModal } from '@/shared/ui'
import { fetchBrowserSessions, type BrowserSessionItem } from '../services/admin-service'

const pageStyle: CSSProperties = { display: 'flex', flexDirection: 'column', gap: 16 }
const cardStyle: CSSProperties = {
  background: 'var(--bg-card)',
  border: '1px solid var(--border-base)',
  borderRadius: 8,
  padding: 16,
}

function isTakeoverSession(item: BrowserSessionItem) {
  return ['human_verification', 'task_takeover'].includes(item.purpose)
}

export default function VerificationSessionsPage({
  targetTaskId,
}: {
  targetTaskId?: string | null
}) {
  const [items, setItems] = useState<BrowserSessionItem[]>([])
  const [activeSession, setActiveSession] = useState<BrowserSession | null>(null)
  const [loading, setLoading] = useState(true)
  const [errorText, setErrorText] = useState('')
  const [cancelConfirmOpen, setCancelConfirmOpen] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    setErrorText('')
    try {
      const sessions = await fetchBrowserSessions()
      setItems(sortSessions(sessions.filter(isTakeoverSession), targetTaskId))
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : '会话加载失败。')
    } finally {
      setLoading(false)
    }
  }, [targetTaskId])

  useEffect(() => void load(), [load])

  useEffect(() => {
    if (!targetTaskId || activeSession) return
    const target = items.find((item) => item.task_id === targetTaskId)
    if (target) void openSession(target)
  }, [activeSession, items, targetTaskId])

  const openSession = async (item: BrowserSessionItem) => {
    setActiveSession(await fetchBrowserSession(item.id))
  }

  const closeSessionWindow = () => {
    setActiveSession(null)
    void load()
  }

  const completeSession = async () => {
    if (!activeSession) return
    await completeBrowserSession(activeSession.id)
    closeSessionWindow()
  }

  const cancelSession = async () => {
    if (!activeSession) return
    await cancelBrowserSession(activeSession.id).catch(() => undefined)
    setCancelConfirmOpen(false)
    closeSessionWindow()
  }

  const sendHistory = async (action: BrowserHistoryAction) => {
    if (!activeSession) return
    await browserSessionHistory(activeSession.id, action).catch(() => undefined)
  }

  return (
    <div style={pageStyle}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <h1 style={{ font: 'var(--font-h1)', color: 'var(--text-bright)' }}>人机接管</h1>
        <KtButton onClick={load}>
          <RotateCw size={16} /> 刷新
        </KtButton>
      </div>
      <div style={cardStyle}>
        <KtAsyncState
          loading={loading}
          error={errorText}
          empty={!loading && items.length === 0}
          emptyText={emptyText(targetTaskId)}
          onRetry={load}
        />
        {!loading &&
          !errorText &&
          items.map((item) => (
            <div key={item.id} style={sessionRowStyle}>
              <span>{item.platform}</span>
              <span>{purposeText(item.purpose)}</span>
              <span>{item.task_id || item.account_id || '-'}</span>
              <KtButton onClick={() => openSession(item)}>打开接管</KtButton>
            </div>
          ))}
      </div>

      <KtModal
        open={!!activeSession}
        onClose={closeSessionWindow}
        title="人机接管"
        titleExtra={activeSession ? <BrowserHistoryControls onAction={sendHistory} /> : null}
        width="min(1180px, calc(100vw - 32px))"
        height="min(860px, calc(100vh - 32px))"
        bodyStyle={{ padding: 0, overflow: 'hidden' }}
        resizable
        closeOnOverlayClick={false}
        footer={
          <>
            <KtButton variant="default" onClick={closeSessionWindow}>
              仅关闭窗口
            </KtButton>
            <KtButton variant="danger" onClick={() => setCancelConfirmOpen(true)}>
              取消会话
            </KtButton>
            <KtButton variant="primary" onClick={completeSession}>
              验证已处理
            </KtButton>
          </>
        }
      >
        <div style={{ height: '100%', minHeight: 0 }}>
          {activeSession ? <RemoteBrowserPanel session={activeSession} mode="login" /> : null}
        </div>
      </KtModal>
      <KtConfirmDialog
        open={cancelConfirmOpen}
        title="取消接管会话"
        message="确定取消当前浏览器接管会话吗？取消后对应任务可能需要重新触发。"
        confirmText="取消会话"
        danger
        onCancel={() => setCancelConfirmOpen(false)}
        onConfirm={cancelSession}
      />
    </div>
  )
}

function emptyText(targetTaskId?: string | null) {
  return targetTaskId
    ? '没有找到这个任务对应的接管会话，请刷新任务或重新执行。'
    : '暂无需要接管的浏览器会话'
}

function sortSessions(items: BrowserSessionItem[], targetTaskId?: string | null) {
  if (!targetTaskId) return items
  return [...items].sort(
    (a, b) => Number(b.task_id === targetTaskId) - Number(a.task_id === targetTaskId)
  )
}

function purposeText(purpose: string) {
  if (purpose === 'task_takeover') return '任务接管'
  if (purpose === 'human_verification') return '人机验证'
  return purpose
}

const sessionRowStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: '1fr 1fr 1.5fr auto',
  gap: 12,
  alignItems: 'center',
  padding: '10px 0',
  borderBottom: '1px solid var(--border-light)',
}

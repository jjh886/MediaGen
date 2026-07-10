import { KtBadge, KtButton } from '@/shared/ui'
import type { Task } from '../types/task'
import {
  attemptActionLabel,
  attemptStatusLabel,
  parseResultData,
  statusVariant,
  statusLabel,
} from '../types/task'

const labelStyle: React.CSSProperties = {
  color: 'var(--text-secondary)',
  fontSize: 13,
  minWidth: 56,
  flexShrink: 0,
}

const valueStyle: React.CSSProperties = {
  color: 'var(--text-primary)',
  fontSize: 13,
  flex: 1,
  wordBreak: 'break-all',
}

const monoValue: React.CSSProperties = {
  ...valueStyle,
  fontFamily: 'monospace',
  fontSize: 12,
}

const rowStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'flex-start',
  gap: 12,
}

const sectionTitle: React.CSSProperties = {
  fontSize: 13,
  fontWeight: 600,
  color: 'var(--text-secondary)',
  marginTop: 8,
}

interface Props {
  task: Task
  accountLabel?: string
  onCancel: (task: Task) => void
  onOpenVerification?: (taskId: string) => void
  onOpenTaskLogs?: (taskId: string) => void
  onClose: () => void
}

function formatDuration(task: Task): string {
  if (!task.completed_at) return '-'
  const ms = new Date(task.completed_at).getTime() - new Date(task.created_at).getTime()
  const sec = Math.round(ms / 1000)
  return sec >= 60 ? `${Math.floor(sec / 60)}分${sec % 60}秒` : `${sec}秒`
}

function shortId(value?: string): string {
  return value ? value.slice(0, 8) : '-'
}

function copyText(value?: string) {
  if (!value) return
  void navigator.clipboard?.writeText(value).catch(() => undefined)
}

function openUrl(value?: string) {
  if (!value) return
  window.open(value, '_blank', 'noopener,noreferrer')
}

function ResultActions({ value, openable }: { value?: string; openable?: boolean }) {
  if (!value) return null
  return (
    <span style={{ display: 'inline-flex', gap: 6, marginLeft: 8 }}>
      <KtButton variant="default" size="sm" onClick={() => copyText(value)}>
        复制
      </KtButton>
      {openable && (
        <KtButton variant="default" size="sm" onClick={() => openUrl(value)}>
          打开
        </KtButton>
      )}
    </span>
  )
}

export default function TaskDetail({
  task,
  accountLabel,
  onCancel,
  onOpenVerification,
  onOpenTaskLogs,
  onClose,
}: Props) {
  const canCancel = task.status === 'pending' || task.status === 'processing'
  const resultData = parseResultData(task)

  return (
    <div
      style={{
        background: 'var(--bg-card)',
        borderRadius: 12,
        padding: 20,
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
        borderTop: '1px solid rgba(255,255,255,0.05)',
        maxHeight: 360,
        overflow: 'auto',
      }}
    >
      {/* 标题栏 */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <span style={{ fontSize: 15, fontWeight: 600, color: 'var(--text-primary)' }}>
          任务详情
        </span>
        <div style={{ display: 'flex', gap: 8 }}>
          {canCancel && (
            <KtButton variant="default" size="sm" onClick={() => onCancel(task)}>
              取消任务
            </KtButton>
          )}
          {task.status === 'verification_required' && (
            <KtButton variant="primary" size="sm" onClick={() => onOpenVerification?.(task.id)}>
              去人机接管
            </KtButton>
          )}
          <KtButton variant="default" size="sm" onClick={() => onOpenTaskLogs?.(task.id)}>
            查看日志
          </KtButton>
          <KtButton variant="default" size="sm" onClick={onClose}>
            关闭
          </KtButton>
        </div>
      </div>

      {/* 请求信息 */}
      <div style={sectionTitle}>请求信息</div>

      <div style={rowStyle}>
        <span style={labelStyle}>ID</span>
        <span style={monoValue}>{task.id}</span>
      </div>

      <div style={rowStyle}>
        <span style={labelStyle}>模型</span>
        <span style={valueStyle}>{task.model_id}</span>
      </div>

      <div style={rowStyle}>
        <span style={labelStyle}>提示词</span>
        <span style={{ ...valueStyle, whiteSpace: 'pre-wrap' }}>{task.prompt || '-'}</span>
      </div>

      {task.account_id && (
        <div style={rowStyle}>
          <span style={labelStyle}>账号</span>
          <span style={valueStyle}>{accountLabel || task.account_id.slice(0, 8)}</span>
        </div>
      )}

      {/* 执行状态 */}
      <div style={sectionTitle}>执行状态</div>

      <div style={rowStyle}>
        <span style={labelStyle}>状态</span>
        <KtBadge variant={statusVariant[task.status]}>{statusLabel[task.status]}</KtBadge>
      </div>

      <div style={rowStyle}>
        <span style={labelStyle}>创建</span>
        <span style={valueStyle}>{new Date(task.created_at).toLocaleString('zh-CN')}</span>
      </div>

      {task.completed_at && (
        <div style={rowStyle}>
          <span style={labelStyle}>完成</span>
          <span style={valueStyle}>
            {new Date(task.completed_at).toLocaleString('zh-CN')}（耗时 {formatDuration(task)}）
          </span>
        </div>
      )}

      {/* 错误信息 */}
      {task.error_message && (
        <>
          <div style={sectionTitle}>错误信息</div>
          <div
            style={{
              background: 'rgba(255,77,79,0.08)',
              border: '1px solid rgba(255,77,79,0.2)',
              borderRadius: 8,
              padding: '10px 14px',
              color: 'var(--color-error)',
              fontSize: 13,
              whiteSpace: 'pre-wrap',
            }}
          >
            {task.error_message}
          </div>
        </>
      )}

      {task.attempts && task.attempts.length > 0 && (
        <>
          <div style={sectionTitle}>尝试记录</div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {task.attempts.map((attempt) => (
              <div
                key={attempt.id}
                style={{
                  display: 'grid',
                  gridTemplateColumns: '52px minmax(84px, 110px) minmax(72px, 1fr)',
                  gap: 8,
                  alignItems: 'center',
                  fontSize: 12,
                  color: 'var(--text-secondary)',
                }}
              >
                <span>第 {attempt.attempt_index} 次</span>
                <KtBadge variant={attempt.status === 'success' ? 'success' : 'warning'}>
                  {attemptStatusLabel[attempt.status] ?? attempt.status}
                </KtBadge>
                <span
                  style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                >
                  {attemptActionLabel[attempt.action] ?? attempt.action} · 账号{' '}
                  {shortId(attempt.account_id)}
                </span>
                {attempt.error_message && (
                  <span
                    style={{
                      gridColumn: '1 / -1',
                      color: 'var(--color-error)',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                    }}
                    title={attempt.error_message}
                  >
                    {attempt.error_code ? `${attempt.error_code}: ` : ''}
                    {attempt.error_message}
                  </span>
                )}
              </div>
            ))}
          </div>
        </>
      )}

      {/* 响应结果 */}
      {(resultData || task.result_url) && (
        <>
          <div style={sectionTitle}>响应结果</div>

          {resultData && (
            <>
              {resultData.url && (
                <div style={rowStyle}>
                  <span style={labelStyle}>URL</span>
                  <span style={monoValue}>
                    {resultData.url}
                    <ResultActions value={resultData.url} openable />
                  </span>
                </div>
              )}
              {(resultData.width || resultData.height) && (
                <div style={rowStyle}>
                  <span style={labelStyle}>尺寸</span>
                  <span style={valueStyle}>
                    {resultData.width} × {resultData.height}
                  </span>
                </div>
              )}
              {resultData.imageCount != null && resultData.imageCount > 0 && (
                <div style={rowStyle}>
                  <span style={labelStyle}>图片数</span>
                  <span style={valueStyle}>{resultData.imageCount}</span>
                </div>
              )}
              {resultData.source && (
                <div style={rowStyle}>
                  <span style={labelStyle}>来源</span>
                  <KtBadge
                    variant={resultData.source === 'sse_interceptor' ? 'success' : 'warning'}
                  >
                    {resultData.source}
                  </KtBadge>
                </div>
              )}
              {resultData.storage_key && (
                <div style={rowStyle}>
                  <span style={labelStyle}>存储</span>
                  <span style={monoValue}>
                    {resultData.storage_key}
                    <ResultActions value={resultData.storage_key} />
                  </span>
                </div>
              )}
            </>
          )}

          {/* 如果没有 resultData 但有 result_url（兼容旧数据） */}
          {!resultData && task.result_url && (
            <div style={rowStyle}>
              <span style={labelStyle}>URL</span>
              <span style={monoValue}>
                {task.result_url}
                <ResultActions value={task.result_url} openable />
              </span>
            </div>
          )}
        </>
      )}
    </div>
  )
}

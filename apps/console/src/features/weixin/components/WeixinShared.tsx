import type { CSSProperties, ReactNode } from 'react'

export const pageStyle: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 16,
  minHeight: 0,
  height: '100%',
}

export const panelGridStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))',
  gap: 16,
  minHeight: 0,
}

const panelStyle: CSSProperties = {
  border: '1px solid var(--border-light)',
  borderRadius: 8,
  background: 'var(--bg-card)',
  padding: 16,
  display: 'flex',
  flexDirection: 'column',
  gap: 14,
}

export function WeixinPanel({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section style={panelStyle}>
      <h2 style={{ color: 'var(--text-bright)', fontSize: 16, lineHeight: '24px' }}>{title}</h2>
      {children}
    </section>
  )
}

export function WeixinField({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: ReactNode
}) {
  return (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <span style={{ color: 'var(--text-secondary)', fontSize: 13 }}>{label}</span>
      {children}
      {hint && <span style={{ color: 'var(--text-tertiary)', fontSize: 12 }}>{hint}</span>}
    </label>
  )
}

export function ToggleRow({
  title,
  description,
  checked,
  onChange,
}: {
  title: string
  description: string
  checked: boolean
  onChange: (checked: boolean) => void
}) {
  return (
    <label
      style={{
        minHeight: 52,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        gap: 14,
        padding: '10px 12px',
        borderRadius: 8,
        background: 'rgba(255,255,255,0.03)',
      }}
    >
      <span style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
        <span style={{ color: 'var(--text-primary)' }}>{title}</span>
        <span style={{ color: 'var(--text-tertiary)', fontSize: 12, lineHeight: '18px' }}>
          {description}
        </span>
      </span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
    </label>
  )
}

export function StatusPill({ status }: { status: string }) {
  const style = statusStyle(status)
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        height: 24,
        padding: '0 10px',
        borderRadius: 999,
        fontSize: 12,
        color: style.color,
        background: style.background,
      }}
    >
      {statusLabel(status)}
    </span>
  )
}

export function formatDateTime(value?: string | null): string {
  if (!value) return '暂无'
  return new Date(value).toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function statusLabel(status: string): string {
  if (status === 'active') return '已绑定'
  if (status === 'pending_activation') return '待激活'
  if (status === 'reauth_required') return '需重登'
  return '未绑定'
}

function statusStyle(status: string) {
  if (status === 'active')
    return { color: 'var(--color-success)', background: 'var(--color-success-bg)' }
  if (status === 'pending_activation') {
    return { color: 'var(--color-warning)', background: 'var(--color-warning-bg)' }
  }
  if (status === 'reauth_required') {
    return { color: 'var(--color-error)', background: 'var(--color-error-bg)' }
  }
  return { color: 'var(--text-secondary)', background: 'rgba(255,255,255,0.06)' }
}

import type { ReactNode } from 'react'
import type { AppSettings } from '@/shared/services/settings-service'
import { cardStyle, titleStyle, Field } from './settings-shared'

export function ApiServiceCard({
  settings: s,
}: {
  settings: AppSettings
  update: <K extends keyof AppSettings>(k: K, v: AppSettings[K]) => void
}) {
  return (
    <div style={cardStyle}>
      <h2 style={titleStyle}>API 服务</h2>
      <Field label="开始端口" hint="当前版本由启动参数控制，只在这里展示。">
        <ReadOnlyValue>{s.api_port}</ReadOnlyValue>
      </Field>
      <Field label="结束端口" hint="当前版本由启动参数控制，只在这里展示。">
        <ReadOnlyValue>{s.api_end_port}</ReadOnlyValue>
      </Field>
      <Field label="监听地址" hint="如需调整，请通过 MEDIAGEN_HOST/MEDIAGEN_PORT 启动参数配置。">
        <ReadOnlyValue>{s.api_address}</ReadOnlyValue>
      </Field>
    </div>
  )
}

function ReadOnlyValue({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        minHeight: 36,
        display: 'flex',
        alignItems: 'center',
        border: '1px solid var(--border-light)',
        borderRadius: 8,
        background: 'var(--bg-elevated)',
        color: 'var(--text-secondary)',
        padding: '0 12px',
      }}
    >
      {children}
    </div>
  )
}

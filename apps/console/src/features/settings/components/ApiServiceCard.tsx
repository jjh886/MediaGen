import type { ReactNode } from 'react'
import type { AppSettings } from '@/shared/services/settings-service'
import { cardStyle, titleStyle, Field } from './settings-shared'

export function ApiServiceCard({ settings: s }: { settings: AppSettings }) {
  return (
    <div style={cardStyle}>
      <h2 style={titleStyle}>API 服务</h2>
      <Field label="服务端口" hint="仅监听一个端口，通过 MEDIAGEN_PORT 环境变量配置，重启后生效。">
        <ReadOnlyValue>{s.api_port}</ReadOnlyValue>
      </Field>
      <Field label="监听地址" hint="默认 0.0.0.0，通过 MEDIAGEN_HOST 环境变量配置，重启后生效。">
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

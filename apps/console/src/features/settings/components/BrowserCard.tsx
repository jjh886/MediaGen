import { KtButton } from '@/shared/ui'
import { cardStyle, titleStyle, Field } from './settings-shared'

export function BrowserCard({
  runtimeVersion,
  detectBrowserRuntime,
}: {
  runtimeVersion: string
  detectBrowserRuntime: () => Promise<void>
}) {
  return (
    <div style={cardStyle}>
      <h2 style={titleStyle}>浏览器</h2>
      <Field label="浏览器运行时" hint="本机安装包会自动携带 CloakBrowser">
        <div style={{ fontSize: 13, color: 'var(--text-secondary)' }}>CloakBrowser 自动检测</div>
      </Field>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <KtButton size="sm" onClick={detectBrowserRuntime}>
          检测
        </KtButton>
        {runtimeVersion && (
          <span style={{ fontSize: 12, color: 'rgba(255,255,255,0.5)' }}>{runtimeVersion}</span>
        )}
      </div>
    </div>
  )
}

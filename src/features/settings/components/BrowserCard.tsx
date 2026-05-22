import { KtButton, KtInput } from '@/shared/ui'
import type { AppSettings } from '@/shared/services/settings-service'
import { cardStyle, titleStyle, Field, strHandler } from './settings-shared'

export function BrowserCard({
  settings: s,
  update,
  chromeVersion,
  detectChrome,
}: {
  settings: AppSettings
  update: <K extends keyof AppSettings>(k: K, v: AppSettings[K]) => void
  chromeVersion: string
  detectChrome: () => Promise<void>
}) {
  return (
    <div style={cardStyle}>
      <h2 style={titleStyle}>浏览器</h2>
      <Field label="Chrome 路径">
        <KtInput
          value={s.chrome_path}
          onChange={strHandler(update, 'chrome_path')}
          placeholder="留空自动检测"
        />
      </Field>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <KtButton size="sm" onClick={detectChrome}>
          检测
        </KtButton>
        {chromeVersion && (
          <span style={{ fontSize: 12, color: 'rgba(255,255,255,0.5)' }}>{chromeVersion}</span>
        )}
      </div>
    </div>
  )
}

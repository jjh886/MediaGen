import { KtInput } from '@/shared/ui'
import type { AppSettings } from '@/shared/services/settings-service'
import { cardStyle, titleStyle, dotStyle, Field, NumInput, strHandler } from './settings-shared'

const runningRowStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  fontSize: 13,
  color: 'rgba(255,255,255,0.6)',
}

function RunningIndicator() {
  return (
    <div style={runningRowStyle}>
      <span style={dotStyle(true)} />
      运行中
    </div>
  )
}

export function ApiServiceCard({
  settings: s,
  update,
}: {
  settings: AppSettings
  update: <K extends keyof AppSettings>(k: K, v: AppSettings[K]) => void
}) {
  return (
    <div style={cardStyle}>
      <h2 style={titleStyle}>API 服务</h2>
      <Field label="开始端口" hint="修改后需重启应用生效">
        <NumInput value={s.api_port} onChange={(v) => update('api_port', v)} />
      </Field>
      <Field label="结束端口" hint="包含该端口">
        <NumInput value={s.api_end_port} onChange={(v) => update('api_end_port', v)} />
      </Field>
      <Field label="监听地址">
        <KtInput value={s.api_address} onChange={strHandler(update, 'api_address')} />
      </Field>
      <Field label="用户 API Key" hint="每行一个；可写成 key=账号组A,账号组B 来限制可用账号组">
        <textarea
          rows={4}
          value={s.api_keys}
          onChange={(event) => update('api_keys', event.target.value)}
          style={{
            width: '100%',
            resize: 'vertical',
            border: '1px solid var(--border-muted)',
            borderRadius: 4,
            background: 'var(--bg-secondary)',
            color: 'var(--text-primary)',
            padding: 8,
          }}
        />
      </Field>
      <RunningIndicator />
    </div>
  )
}

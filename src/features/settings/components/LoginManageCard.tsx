import type { AppSettings } from '@/shared/services/settings-service'
import { cardStyle, titleStyle, Field, NumInput } from './settings-shared'

export function LoginManageCard({
  settings: s,
  update,
}: {
  settings: AppSettings
  update: <K extends keyof AppSettings>(k: K, v: AppSettings[K]) => void
}) {
  return (
    <div style={cardStyle}>
      <h2 style={titleStyle}>登录管理</h2>
      <Field label="检查间隔（分钟）">
        <NumInput
          value={s.check_interval_minutes}
          onChange={(v) => update('check_interval_minutes', v)}
        />
      </Field>
      <Field label="冷却时间（秒）">
        <NumInput
          value={s.cooldown_default_seconds}
          onChange={(v) => update('cooldown_default_seconds', v)}
        />
      </Field>
    </div>
  )
}

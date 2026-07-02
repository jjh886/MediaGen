import { KtButton, KtInput } from '@/shared/ui'
import type { AppSettings } from '@/shared/services/settings-service'
import { cardStyle, titleStyle, Field, strHandler } from './settings-shared'

export function MirrorCard({
  settings: s,
  update,
}: {
  settings: AppSettings
  update: <K extends keyof AppSettings>(k: K, v: AppSettings[K]) => void
}) {
  const checkReachability = async () => {
    if (!s.flow_mirror_url) return
    try {
      await fetch(s.flow_mirror_url, { mode: 'no-cors', signal: AbortSignal.timeout(5000) })
      alert('镜像站可达')
    } catch {
      alert('镜像站不可达，请检查地址')
    }
  }

  return (
    <div style={cardStyle}>
      <h2 style={titleStyle}>平台镜像</h2>
      <Field label="镜像地址">
        <KtInput
          value={s.flow_mirror_url}
          onChange={strHandler(update, 'flow_mirror_url')}
          placeholder="https://..."
        />
      </Field>
      <KtButton size="sm" onClick={checkReachability}>
        检测可达性
      </KtButton>
    </div>
  )
}

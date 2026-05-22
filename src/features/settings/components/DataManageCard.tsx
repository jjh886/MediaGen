import { KtButton, KtInput } from '@/shared/ui'
import type { AppSettings } from '@/shared/services/settings-service'
import { cardStyle, titleStyle, Field } from './settings-shared'

export function DataManageCard({
  settings: s,
  cacheSize,
  cleanCache,
  refreshCacheSize,
}: {
  settings: AppSettings
  cacheSize: string
  cleanCache: () => Promise<void>
  refreshCacheSize: () => Promise<void>
}) {
  return (
    <div style={{ ...cardStyle, flexDirection: 'row', alignItems: 'center', gap: 24 }}>
      <h2 style={{ ...titleStyle, flexShrink: 0 }}>数据管理</h2>
      <Field label="缓存目录">
        <KtInput value={s.cache_dir} readOnly style={{ maxWidth: 320 }} />
      </Field>
      <div style={{ fontSize: 13, color: 'rgba(255,255,255,0.6)', whiteSpace: 'nowrap' }}>
        占用：{cacheSize || '—'}
      </div>
      <KtButton size="sm" variant="danger" onClick={cleanCache}>
        清理缓存
      </KtButton>
      <KtButton size="sm" onClick={refreshCacheSize}>
        刷新
      </KtButton>
    </div>
  )
}

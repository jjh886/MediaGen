import { useEffect } from 'react'
import { KtButton } from '@/shared/ui'
import { useSettingsStore } from '../hooks/useSettingsStore'
import { ApiServiceCard } from './ApiServiceCard'
import { MirrorCard } from './MirrorCard'
import { BrowserCard } from './BrowserCard'
import { LoginManageCard } from './LoginManageCard'
import { AutoRegisterCard } from './AutoRegisterCard'
import { PlatformQuotaCard } from './PlatformQuotaCard'
import { DataManageCard } from './DataManageCard'

export function SettingsPage() {
  const {
    settings,
    loading,
    saving,
    chromeVersion,
    cacheSize,
    load,
    update,
    save,
    detectChrome,
    refreshCacheSize,
    cleanCache,
  } = useSettingsStore()

  useEffect(() => {
    load()
  }, [load])

  if (loading || !settings) {
    return <div style={{ padding: 32, color: 'rgba(255,255,255,0.4)' }}>加载中...</div>
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16, height: '100%' }}>
      {/* 头部 */}
      <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
        <KtButton variant="primary" loading={saving} onClick={save}>
          保存设置
        </KtButton>
      </div>

      {/* 2 列网格 */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: '1fr 1fr',
          gap: 24,
          flex: 1,
          overflow: 'auto',
        }}
      >
        <ApiServiceCard settings={settings} update={update} />
        <MirrorCard settings={settings} update={update} />
        <BrowserCard
          settings={settings}
          update={update}
          chromeVersion={chromeVersion}
          detectChrome={detectChrome}
        />
        <LoginManageCard settings={settings} update={update} />
        <AutoRegisterCard settings={settings} update={update} />
        <div style={{ gridColumn: '1 / -1' }}>
          <PlatformQuotaCard settings={settings} update={update} />
        </div>
      </div>

      {/* 数据管理 — 全宽 */}
      <DataManageCard
        settings={settings}
        cacheSize={cacheSize}
        cleanCache={cleanCache}
        refreshCacheSize={refreshCacheSize}
      />
    </div>
  )
}

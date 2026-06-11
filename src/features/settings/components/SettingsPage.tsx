import { useEffect } from 'react'
import { KtButton } from '@/shared/ui'
import type { AppSettings } from '@/shared/services/settings-service'
import { useSettingsStore } from '../hooks/useSettingsStore'
import { ApiServiceCard } from './ApiServiceCard'
import { MirrorCard } from './MirrorCard'
import { BrowserCard } from './BrowserCard'
import { LoginManageCard } from './LoginManageCard'
import { PlatformQuotaCard } from './PlatformQuotaCard'
import { DataManageCard } from './DataManageCard'

type SettingsUpdate = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void

function SettingsToolbar({ saving, save }: { saving: boolean; save: () => Promise<void> }) {
  return (
    <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
      <KtButton variant="primary" loading={saving} onClick={save}>
        保存设置
      </KtButton>
    </div>
  )
}

function SettingsCardGrid({
  settings,
  chromeVersion,
  update,
  detectChrome,
}: {
  settings: AppSettings
  chromeVersion: string
  update: SettingsUpdate
  detectChrome: () => Promise<void>
}) {
  return (
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
      <div style={{ gridColumn: '1 / -1' }}>
        <PlatformQuotaCard settings={settings} update={update} />
      </div>
    </div>
  )
}

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
      <SettingsToolbar saving={saving} save={save} />
      <SettingsCardGrid
        settings={settings}
        chromeVersion={chromeVersion}
        update={update}
        detectChrome={detectChrome}
      />
      <DataManageCard
        settings={settings}
        cacheSize={cacheSize}
        cleanCache={cleanCache}
        refreshCacheSize={refreshCacheSize}
      />
    </div>
  )
}

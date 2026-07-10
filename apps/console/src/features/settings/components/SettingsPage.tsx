import { useEffect } from 'react'
import { KtButton } from '@/shared/ui'
import type { AppSettings } from '@/shared/services/settings-service'
import { useSettingsStore } from '../hooks/useSettingsStore'
import { ApiServiceCard } from './ApiServiceCard'
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

function SettingsCardGrid({ settings, update }: { settings: AppSettings; update: SettingsUpdate }) {
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
    loadError,
    saveError,
    saveMessage,
    cacheSize,
    load,
    update,
    save,
    refreshCacheSize,
    cleanCache,
  } = useSettingsStore()

  useEffect(() => {
    load()
  }, [load])

  if (loading) {
    return <div style={{ padding: 32, color: 'rgba(255,255,255,0.4)' }}>加载中...</div>
  }

  if (loadError || !settings) {
    return (
      <div style={{ padding: 32, color: 'var(--color-error)' }}>
        设置加载失败：{loadError || '服务没有返回设置数据'}
      </div>
    )
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16, height: '100%' }}>
      <SettingsToolbar saving={saving} save={save} />
      {(saveError || saveMessage) && (
        <div
          style={{
            color: saveError ? 'var(--color-error)' : 'var(--color-success)',
            fontSize: 13,
          }}
        >
          {saveError || saveMessage}
        </div>
      )}
      <SettingsCardGrid settings={settings} update={update} />
      <DataManageCard
        settings={settings}
        cacheSize={cacheSize}
        cleanCache={cleanCache}
        refreshCacheSize={refreshCacheSize}
      />
    </div>
  )
}

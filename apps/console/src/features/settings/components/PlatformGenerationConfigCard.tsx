import { useEffect, useState } from 'react'
import { KtInput, KtSelect } from '@/shared/ui'
import type { AppSettings } from '@/shared/services/settings-service'
import {
  fetchPlugins,
  type PluginGenerationSettingField,
  type PluginInfo,
} from '@/shared/services/plugin-service'
import { cardStyle, Field, titleStyle } from './settings-shared'
import {
  readPlatformGenerationConfig,
  writePlatformGenerationConfig,
  type PlatformGenerationConfig,
} from './platform-generation-config'

type SettingsUpdate = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void

export function PlatformGenerationConfigCard({
  settings,
  update,
}: {
  settings: AppSettings
  update: SettingsUpdate
}) {
  const [plugins, setPlugins] = useState<PluginInfo[]>([])

  useEffect(() => {
    fetchPlugins()
      .then(setPlugins)
      .catch(() => setPlugins([]))
  }, [])

  return (
    <div style={{ display: 'grid', gap: 16 }}>
      {plugins
        .filter((plugin) => plugin.generation_settings)
        .map((plugin) => (
          <PluginGenerationConfig
            key={plugin.platform_id}
            plugin={plugin}
            raw={settings.platform_generation_configs}
            update={update}
          />
        ))}
    </div>
  )
}

function PluginGenerationConfig({
  plugin,
  raw,
  update,
}: {
  plugin: PluginInfo
  raw: string
  update: SettingsUpdate
}) {
  const schema = plugin.generation_settings!
  const config = readPlatformGenerationConfig(raw, plugin.platform_id, schema)
  const change = (next: PlatformGenerationConfig) => {
    update(
      'platform_generation_configs',
      writePlatformGenerationConfig(raw, plugin.platform_id, next)
    )
  }

  return (
    <div style={cardStyle}>
      <h2 style={titleStyle}>{schema.title}</h2>
      {schema.description && (
        <div style={{ fontSize: 13, color: 'var(--text-secondary)' }}>{schema.description}</div>
      )}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))',
          gap: 16,
        }}
      >
        {schema.fields.map((field) => (
          <GenerationConfigField
            key={field.key}
            pluginName={plugin.display_name}
            field={field}
            value={config[field.key]}
            change={(value) => change({ ...config, [field.key]: value })}
          />
        ))}
      </div>
      {schema.notice && (
        <div style={{ fontSize: 12, color: 'var(--text-disabled)' }}>{schema.notice}</div>
      )}
    </div>
  )
}

function GenerationConfigField({
  pluginName,
  field,
  value,
  change,
}: {
  pluginName: string
  field: PluginGenerationSettingField
  value: string
  change: (value: string) => void
}) {
  const label = `${pluginName} ${field.label}`
  return (
    <Field label={field.label} hint={field.hint}>
      {field.type === 'select' ? (
        <KtSelect
          aria-label={label}
          value={value}
          options={field.options || []}
          onChange={change}
        />
      ) : (
        <KtInput
          aria-label={label}
          value={value}
          placeholder={field.placeholder}
          onChange={(event) => change(event.target.value)}
        />
      )}
    </Field>
  )
}

import { KtInput } from '@/shared/ui'
import type { AppSettings } from '@/shared/services/settings-service'
import { cardStyle, titleStyle } from './settings-shared'
import flowManifest from '../../../../../../plugins/flow/manifest.json'
import doubaoManifest from '../../../../../../plugins/doubao/manifest.json'
import chatgptManifest from '../../../../../../plugins/chatgpt/manifest.json'
import codexManifest from '../../../../../../plugins/codex/manifest.json'
import jimengManifest from '../../../../../../plugins/jimeng/manifest.json'

type TierOverride = {
  daily_limit?: number
  total_limit?: number
  concurrency_limit?: number
  task_interval_seconds?: number
}

type TierDefaults = {
  daily_limit: number
  total_limit: number
  concurrency_limit: number
  task_interval_seconds: number
}

type TierField = keyof TierOverride

type PluginQuotaManifest = {
  platform_id: string
  display_name: string
  tiers: {
    free: TierDefaults
  }
}

const PLATFORM_QUOTA_ROWS = (
  [
    flowManifest,
    doubaoManifest,
    chatgptManifest,
    codexManifest,
    jimengManifest,
  ] as PluginQuotaManifest[]
).map((manifest) => ({
  label: `${manifest.display_name} 免费`,
  tierKey: `${manifest.platform_id}_free`,
  defaults: manifest.tiers.free,
}))

function parseTierOverrides(overrides: string): Record<string, TierOverride> {
  try {
    return JSON.parse(overrides || '{}')
  } catch {
    return {}
  }
}

function resolveQuotaValues(current: TierOverride, defaults: TierDefaults): TierDefaults {
  return {
    daily_limit: current.daily_limit ?? defaults.daily_limit,
    total_limit: current.total_limit ?? defaults.total_limit,
    concurrency_limit: current.concurrency_limit ?? defaults.concurrency_limit,
    task_interval_seconds: current.task_interval_seconds ?? defaults.task_interval_seconds,
  }
}

function updateTierOverride(
  parsed: Record<string, TierOverride>,
  tierKey: string,
  field: TierField,
  value: number
): string {
  const current = parsed[tierKey] || {}
  return JSON.stringify({ ...parsed, [tierKey]: { ...current, [field]: value } })
}

function QuotaNumberInput({
  label,
  value,
  onChange,
}: {
  label: string
  value: number
  onChange: (v: number) => void
}) {
  return (
    <>
      <span style={{ fontSize: 12, color: 'var(--text-secondary)' }}>{label}</span>
      <KtInput
        type="number"
        value={String(value)}
        onChange={(e) => onChange(Number(e.target.value) || 0)}
        style={{ width: 70 }}
      />
    </>
  )
}

function PlatformQuotaRow({
  label,
  tierKey,
  overrides,
  onChange,
  defaults,
}: {
  label: string
  tierKey: string
  overrides: string
  onChange: (v: string) => void
  defaults: TierDefaults
}) {
  const parsed = parseTierOverrides(overrides)
  const current = parsed[tierKey] || {}
  const values = resolveQuotaValues(current, defaults)

  const setField = (field: TierField, v: number) => {
    onChange(updateTierOverride(parsed, tierKey, field, v))
  }

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
      <span style={{ fontSize: 13, color: 'var(--text-primary)', width: 100, flexShrink: 0 }}>
        {label}
      </span>
      <QuotaNumberInput
        label="日限"
        value={values.daily_limit}
        onChange={(v) => setField('daily_limit', v)}
      />
      <QuotaNumberInput
        label="总限"
        value={values.total_limit}
        onChange={(v) => setField('total_limit', v)}
      />
      <QuotaNumberInput
        label="并发"
        value={values.concurrency_limit}
        onChange={(v) => setField('concurrency_limit', v)}
      />
      <QuotaNumberInput
        label="间隔"
        value={values.task_interval_seconds}
        onChange={(v) => setField('task_interval_seconds', v)}
      />
      <span style={{ fontSize: 11, color: 'var(--text-disabled)' }}>
        限额 0=无限，间隔 0=不等待
      </span>
    </div>
  )
}

export function PlatformQuotaCard({
  settings: s,
  update,
}: {
  settings: AppSettings
  update: <K extends keyof AppSettings>(k: K, v: AppSettings[K]) => void
}) {
  const handleChange = (v: string) => update('platform_tier_overrides', v)

  return (
    <div style={cardStyle}>
      <h2 style={titleStyle}>平台默认配额与间隔</h2>
      <div style={{ fontSize: 13, color: 'var(--text-secondary)', marginBottom: 4 }}>
        新增账号和同档位已有账号会使用这里的值；任务间隔用于避免同一账号连续开启新任务。
      </div>
      {PLATFORM_QUOTA_ROWS.map((row) => (
        <PlatformQuotaRow
          key={row.tierKey}
          label={row.label}
          tierKey={row.tierKey}
          overrides={s.platform_tier_overrides}
          onChange={handleChange}
          defaults={row.defaults}
        />
      ))}
    </div>
  )
}

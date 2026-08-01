import type {
  PluginGenerationSettingField,
  PluginGenerationSettings,
} from '@/shared/services/plugin-service'

export type PlatformGenerationConfig = Record<string, string>

export function readPlatformGenerationConfig(
  raw: string,
  platformId: string,
  schema: PluginGenerationSettings
): PlatformGenerationConfig {
  const saved = asRecord(parseRoot(raw)[platformId])
  return Object.fromEntries(
    schema.fields.map((field) => [field.key, normalizedFieldValue(saved[field.key], field)])
  )
}

export function writePlatformGenerationConfig(
  raw: string,
  platformId: string,
  config: PlatformGenerationConfig
): string {
  const root = parseRoot(raw)
  // 每次只替换当前插件的配置，其他插件已经保存的参数必须完整保留。
  return JSON.stringify({ ...root, [platformId]: config })
}

function normalizedFieldValue(value: unknown, field: PluginGenerationSettingField): string {
  if (typeof value !== 'string') return field.default
  if (field.type !== 'select') return value
  return field.options?.some((option) => option.value === value) ? value : field.default
}

function parseRoot(raw: string): Record<string, unknown> {
  try {
    return asRecord(JSON.parse(raw || '{}'))
  } catch {
    return {}
  }
}

function asRecord(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {}
}

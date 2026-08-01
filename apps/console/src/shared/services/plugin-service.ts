import { apiGet } from './http-api-client'

export interface PluginInfo {
  id: string
  platform_id: string
  display_name: string
  model_count: number
  supports_registration: boolean
  generation_settings?: PluginGenerationSettings
}

export interface PluginGenerationSettingOption {
  value: string
  label: string
}

export interface PluginGenerationSettingField {
  key: string
  type: 'text' | 'select'
  label: string
  hint?: string
  placeholder?: string
  default: string
  options?: PluginGenerationSettingOption[]
}

export interface PluginGenerationSettings {
  title: string
  description?: string
  notice?: string
  fields: PluginGenerationSettingField[]
}

export interface PluginTierQuota {
  daily_limit: number
  total_limit: number
  concurrency_limit: number
  task_interval_seconds: number
}

export interface PluginQuotaInfo {
  platform_id: string
  display_name: string
  tiers: Record<string, PluginTierQuota>
}

export async function fetchPlugins(): Promise<PluginInfo[]> {
  const plugins = await apiGet<(PluginInfo & { models?: unknown[] })[]>('/api/plugins')
  return plugins.map((plugin) => ({
    id: plugin.platform_id,
    platform_id: plugin.platform_id,
    display_name: plugin.display_name,
    model_count: plugin.model_count ?? plugin.models?.length ?? 0,
    supports_registration: plugin.supports_registration ?? false,
    generation_settings: normalizeGenerationSettings(plugin.generation_settings),
  }))
}

export async function fetchPluginQuotas(): Promise<PluginQuotaInfo[]> {
  return await apiGet<PluginQuotaInfo[]>('/api/plugins')
}

function normalizeGenerationSettings(value: unknown): PluginGenerationSettings | undefined {
  const schema = asRecord(value)
  if (typeof schema.title !== 'string' || !Array.isArray(schema.fields)) return undefined
  const fields = schema.fields.map(normalizeGenerationField).filter((field) => field !== undefined)
  if (!fields.length) return undefined
  return {
    title: schema.title,
    description: optionalString(schema.description),
    notice: optionalString(schema.notice),
    fields,
  }
}

function normalizeGenerationField(value: unknown): PluginGenerationSettingField | undefined {
  const field = asRecord(value)
  if (
    typeof field.key !== 'string' ||
    !/^[a-z][a-z0-9_]{0,63}$/.test(field.key) ||
    (field.type !== 'text' && field.type !== 'select') ||
    typeof field.label !== 'string'
  ) {
    return undefined
  }
  const options = Array.isArray(field.options)
    ? field.options.map(normalizeGenerationOption).filter((option) => option !== undefined)
    : undefined
  if (field.type === 'select' && !options?.length) return undefined
  return {
    key: field.key,
    type: field.type,
    label: field.label,
    hint: optionalString(field.hint),
    placeholder: optionalString(field.placeholder),
    default: typeof field.default === 'string' ? field.default : '',
    options,
  }
}

function normalizeGenerationOption(value: unknown): PluginGenerationSettingOption | undefined {
  const option = asRecord(value)
  return typeof option.value === 'string' && typeof option.label === 'string'
    ? { value: option.value, label: option.label }
    : undefined
}

function asRecord(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {}
}

function optionalString(value: unknown): string | undefined {
  return typeof value === 'string' && value ? value : undefined
}

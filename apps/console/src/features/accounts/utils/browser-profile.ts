export type BrowserOs = 'windows' | 'macos' | 'linux'

export interface BrowserProfileFormState {
  enabled: boolean
  os: BrowserOs
}

export interface BrowserProfilePayload {
  browser_os: string
  browser_version: string
}

export const emptyBrowserProfileForm: BrowserProfileFormState = {
  enabled: false,
  os: 'windows',
}

const supportedBrowserOs: BrowserOs[] = ['windows', 'macos', 'linux']

// 2026-07-13：Chrome 官方为三个桌面系统发布同一稳定主版本；分开保存方便以后独立更新。
const defaultChromeMajorVersionByOs: Record<BrowserOs, string> = {
  windows: '150',
  macos: '150',
  linux: '150',
}

export function parseBrowserProfile(
  browserOs?: string | null,
  browserVersion?: string | null
): BrowserProfileFormState {
  const os = normalizeBrowserOs(browserOs)
  const version = browserVersion?.trim() ?? ''
  return {
    enabled: Boolean(browserOs?.trim() || version),
    os,
  }
}

export function buildBrowserProfilePayload(form: BrowserProfileFormState): BrowserProfilePayload {
  if (!form.enabled) return { browser_os: '', browser_version: '' }
  return { browser_os: form.os, browser_version: getDefaultChromeMajorVersion(form.os) }
}

export function getDefaultChromeMajorVersion(os: BrowserOs): string {
  return defaultChromeMajorVersionByOs[os]
}

function normalizeBrowserOs(value?: string | null): BrowserOs {
  const normalized = value?.trim().toLowerCase()
  if (supportedBrowserOs.includes(normalized as BrowserOs)) return normalized as BrowserOs
  return 'windows'
}

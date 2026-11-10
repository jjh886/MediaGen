export type BrowserOs = 'windows' | 'macos' | 'linux'

export interface BrowserProfileFormState {
  enabled: boolean
  os: BrowserOs
  version: string
}

export interface BrowserProfilePayload {
  browser_os: string
  browser_version: string
}

export const emptyBrowserProfileForm: BrowserProfileFormState = {
  enabled: true,
  os: 'windows',
  version: '',
}

const supportedBrowserOs: BrowserOs[] = ['windows', 'macos', 'linux']

export function parseBrowserProfile(
  browserOs?: string | null,
  browserVersion?: string | null
): BrowserProfileFormState {
  const os = normalizeBrowserOs(browserOs)
  const version = browserVersion?.trim() ?? ''
  return {
    enabled: Boolean(browserOs?.trim() || version),
    os,
    version,
  }
}

export function buildBrowserProfilePayload(form: BrowserProfileFormState): BrowserProfilePayload {
  if (!form.enabled) return { browser_os: '', browser_version: '' }
  if (!/^\d{2,3}$/.test(form.version)) throw new Error('请先选择浏览器版本。')
  return { browser_os: form.os, browser_version: form.version }
}

export function browserVersionChoices(latestVersion: string, supported: string[]): string[] {
  const major = Number(latestVersion)
  if (!Number.isInteger(major) || major < 100 || major > 999) return []
  return [...new Set(supported)]
    .filter((version) => /^\d{3}$/.test(version) && Number(version) <= major)
    .sort((a, b) => Number(b) - Number(a))
    .slice(0, 3)
}

function normalizeBrowserOs(value?: string | null): BrowserOs {
  const normalized = value?.trim().toLowerCase()
  if (supportedBrowserOs.includes(normalized as BrowserOs)) return normalized as BrowserOs
  return 'windows'
}

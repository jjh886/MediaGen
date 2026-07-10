export type BrowserOs = 'windows' | 'macos' | 'linux'

export interface BrowserProfileFormState {
  enabled: boolean
  os: BrowserOs
  browserVersion: string
}

export interface BrowserProfilePayload {
  browser_os: string
  browser_version: string
}

export const emptyBrowserProfileForm: BrowserProfileFormState = {
  enabled: false,
  os: 'windows',
  browserVersion: '',
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
    browserVersion: version,
  }
}

export function buildBrowserProfilePayload(form: BrowserProfileFormState): BrowserProfilePayload {
  if (!form.enabled) return { browser_os: '', browser_version: '' }
  const version = normalizeChromeMajorVersion(form.browserVersion)
  if (!version) {
    throw new Error('请填写 Chrome 主版本号，例如 126。')
  }
  return { browser_os: form.os, browser_version: version }
}

function normalizeBrowserOs(value?: string | null): BrowserOs {
  const normalized = value?.trim().toLowerCase()
  if (supportedBrowserOs.includes(normalized as BrowserOs)) return normalized as BrowserOs
  return 'windows'
}

function normalizeChromeMajorVersion(value: string): string {
  const version = value.trim()
  if (!/^\d{1,3}$/.test(version)) return ''
  const major = Number(version)
  return Number.isInteger(major) && major > 0 ? String(major) : ''
}

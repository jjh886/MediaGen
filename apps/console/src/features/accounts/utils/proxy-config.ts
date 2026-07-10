import {
  buildProxyServerUrl,
  parseProxyUrlParts,
  type ProxyProtocol,
} from '@/shared/utils/proxy-url'

export type { ProxyProtocol }

export interface ProxyFormState {
  enabled: boolean
  protocol: ProxyProtocol
  host: string
  port: string
  username: string
  password: string
}

export const emptyProxyForm: ProxyFormState = {
  enabled: false,
  protocol: 'socks5h',
  host: '',
  port: '',
  username: '',
  password: '',
}

export function parseProxyUrl(raw?: string | null): ProxyFormState {
  const text = raw?.trim() ?? ''
  if (!text) return { ...emptyProxyForm }

  const parsed = parseProxyUrlParts(text)
  if (parsed) {
    return {
      enabled: true,
      ...parsed,
    }
  }
  return { ...emptyProxyForm, enabled: true, host: text }
}

export function buildProxyUrl(form: ProxyFormState): string {
  if (!form.enabled) return ''
  return buildProxyServerUrl(form)
}

export function apiKeyLines(raw: string): string[] {
  return raw
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean)
}

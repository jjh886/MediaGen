export type ProxyProtocol = 'socks5' | 'socks5h'

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

const supportedProtocols: ProxyProtocol[] = ['socks5', 'socks5h']

export function parseProxyUrl(raw?: string | null): ProxyFormState {
  const text = raw?.trim() ?? ''
  if (!text) return { ...emptyProxyForm }

  const normalized = text.includes('://') ? text : `socks5h://${text}`
  try {
    const url = new URL(normalized)
    const protocol = url.protocol.replace(':', '') as ProxyProtocol
    if (!supportedProtocols.includes(protocol)) return { ...emptyProxyForm }
    return {
      enabled: true,
      protocol,
      host: url.hostname,
      port: url.port,
      username: decodeURIComponent(url.username),
      password: decodeURIComponent(url.password),
    }
  } catch {
    return { ...emptyProxyForm, enabled: true, host: text }
  }
}

export function buildProxyUrl(form: ProxyFormState): string {
  if (!form.enabled) return ''
  const host = form.host.trim()
  const port = Number(form.port)
  if (!host) throw new Error('请填写代理地址。')
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error('请填写 1 到 65535 之间的代理端口。')
  }
  if (form.password.trim() && !form.username.trim()) {
    throw new Error('代理填写了密码时，也需要填写账号。')
  }

  // URL 里只编码认证信息；host 保持用户输入，方便支持域名和 IP。
  const auth = form.username.trim()
    ? `${encodeURIComponent(form.username.trim())}:${encodeURIComponent(form.password)}@`
    : ''
  return `${form.protocol}://${auth}${host}:${port}`
}

export function apiKeyLines(raw: string): string[] {
  return raw
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean)
}

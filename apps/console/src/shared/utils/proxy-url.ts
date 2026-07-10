export type ProxyProtocol = 'socks5' | 'socks5h'

export interface ProxyUrlParts {
  protocol: ProxyProtocol
  host: string
  port: string
  username: string
  password: string
}

const supportedProtocols: ProxyProtocol[] = ['socks5', 'socks5h']

export function parseProxyUrlParts(raw: string): ProxyUrlParts | null {
  const text = raw.trim()
  if (!text) return null
  const normalized = text.includes('://') ? text : `socks5h://${text}`
  try {
    const url = new URL(normalized)
    const protocol = url.protocol.replace(':', '') as ProxyProtocol
    if (!supportedProtocols.includes(protocol) || !url.hostname) return null
    return {
      protocol,
      host: url.hostname,
      port: url.port,
      username: decodeURIComponent(url.username),
      password: decodeURIComponent(url.password),
    }
  } catch {
    return null
  }
}

export function buildProxyServerUrl(parts: ProxyUrlParts): string {
  const host = parts.host.trim()
  const port = Number(parts.port)
  if (!host) throw new Error('请填写代理地址。')
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error('请填写 1 到 65535 之间的代理端口。')
  }
  if (parts.password.trim() && !parts.username.trim()) {
    throw new Error('代理填写了密码时，也需要填写账号。')
  }
  const auth = parts.username.trim()
    ? `${encodeURIComponent(parts.username.trim())}:${encodeURIComponent(parts.password)}@`
    : ''
  return `${parts.protocol}://${auth}${host}:${port}`
}

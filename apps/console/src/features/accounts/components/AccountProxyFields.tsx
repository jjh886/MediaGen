import { KtInput, KtSelect } from '@/shared/ui'
import type { ProxyFormState, ProxyProtocol } from '../utils/proxy-config'

interface AccountProxyFieldsProps {
  value: ProxyFormState
  onChange: (value: ProxyFormState) => void
}

const protocolOptions: { value: ProxyProtocol; label: string }[] = [
  { value: 'socks5h', label: 'SOCKS5H（推荐）' },
  { value: 'socks5', label: 'SOCKS5' },
]

const gridStyle: React.CSSProperties = {
  display: 'grid',
  gridTemplateColumns: '1fr 120px',
  gap: 10,
}

const labelStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  color: 'var(--text-secondary)',
  fontSize: 13,
}

const fieldStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 6,
}

function updateField<K extends keyof ProxyFormState>(
  value: ProxyFormState,
  key: K,
  next: ProxyFormState[K]
) {
  return { ...value, [key]: next }
}

export default function AccountProxyFields({ value, onChange }: AccountProxyFieldsProps) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <label style={labelStyle}>
        <input
          type="checkbox"
          checked={value.enabled}
          onChange={(event) => onChange(updateField(value, 'enabled', event.target.checked))}
        />
        为这个账号单独设置代理
      </label>

      {value.enabled && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div style={gridStyle}>
            <div style={fieldStyle}>
              <span style={labelStyle}>代理协议</span>
              <KtSelect
                value={value.protocol}
                onChange={(protocol) =>
                  onChange(updateField(value, 'protocol', protocol as ProxyProtocol))
                }
                options={protocolOptions}
              />
            </div>
            <div style={fieldStyle}>
              <span style={labelStyle}>端口</span>
              <KtInput
                type="number"
                min={1}
                max={65535}
                value={value.port}
                onChange={(event) => onChange(updateField(value, 'port', event.target.value))}
              />
            </div>
          </div>

          <div style={fieldStyle}>
            <span style={labelStyle}>代理地址</span>
            <KtInput
              placeholder="127.0.0.1 或 proxy.example.com"
              value={value.host}
              onChange={(event) => onChange(updateField(value, 'host', event.target.value))}
            />
          </div>

          <div style={gridStyle}>
            <div style={fieldStyle}>
              <span style={labelStyle}>账号</span>
              <KtInput
                value={value.username}
                onChange={(event) => onChange(updateField(value, 'username', event.target.value))}
              />
            </div>
            <div style={fieldStyle}>
              <span style={labelStyle}>密码</span>
              <KtInput
                type="password"
                value={value.password}
                onChange={(event) => onChange(updateField(value, 'password', event.target.value))}
              />
            </div>
          </div>
          <span style={{ color: 'var(--text-disabled)', fontSize: 12 }}>
            开启后，登录窗口、浏览器任务和插件 HTTP 请求都会使用这个代理。
          </span>
        </div>
      )}
    </div>
  )
}

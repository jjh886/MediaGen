import { KtInput, KtSelect } from '@/shared/ui'
import { parseProxyUrlParts } from '@/shared/utils/proxy-url'
import type { ProxyFormState, ProxyProtocol } from '../utils/proxy-config'
import {
  accountFieldHintStyle,
  accountFieldLabelStyle,
  accountFieldStyle,
  accountFormStackStyle,
  accountTwoColumnGridStyle,
  updateAccountFormField,
} from './account-form-shared'
import AccountSettingSwitch from './AccountSettingSwitch'

interface AccountProxyFieldsProps {
  value: ProxyFormState
  onChange: (value: ProxyFormState) => void
}

const protocolOptions: { value: ProxyProtocol; label: string }[] = [
  { value: 'http', label: 'HTTP' },
  { value: 'https', label: 'HTTPS' },
  { value: 'socks5h', label: 'SOCKS5H' },
  { value: 'socks5', label: 'SOCKS5' },
]

const proxyGridStyle = accountTwoColumnGridStyle(120)

function applyHostInput(value: ProxyFormState, raw: string): ProxyFormState {
  const parsed = parseProxyUrlParts(raw)
  if (!parsed || (!raw.includes('://') && !raw.includes('@'))) {
    return updateAccountFormField(value, 'host', raw)
  }
  return { ...value, enabled: true, ...parsed }
}

export default function AccountProxyFields({ value, onChange }: AccountProxyFieldsProps) {
  return (
    <div style={accountFormStackStyle}>
      <AccountSettingSwitch
        checked={value.enabled}
        onChange={(enabled) => onChange(updateAccountFormField(value, 'enabled', enabled))}
      >
        为这个账号单独设置代理
      </AccountSettingSwitch>

      {value.enabled && (
        <div style={accountFormStackStyle}>
          <div style={proxyGridStyle}>
            <div style={accountFieldStyle}>
              <span style={accountFieldLabelStyle}>代理协议</span>
              <KtSelect
                value={value.protocol}
                onChange={(protocol) =>
                  onChange(updateAccountFormField(value, 'protocol', protocol as ProxyProtocol))
                }
                options={protocolOptions}
              />
            </div>
            <div style={accountFieldStyle}>
              <span style={accountFieldLabelStyle}>端口</span>
              <KtInput
                type="number"
                min={1}
                max={65535}
                value={value.port}
                onChange={(event) =>
                  onChange(updateAccountFormField(value, 'port', event.target.value))
                }
              />
            </div>
          </div>

          <div style={accountFieldStyle}>
            <span style={accountFieldLabelStyle}>代理地址</span>
            <KtInput
              placeholder="主机、完整代理 URL 或 username:password@ip:port"
              value={value.host}
              onChange={(event) => onChange(applyHostInput(value, event.target.value))}
            />
          </div>

          <div style={proxyGridStyle}>
            <div style={accountFieldStyle}>
              <span style={accountFieldLabelStyle}>账号</span>
              <KtInput
                value={value.username}
                onChange={(event) =>
                  onChange(updateAccountFormField(value, 'username', event.target.value))
                }
              />
            </div>
            <div style={accountFieldStyle}>
              <span style={accountFieldLabelStyle}>密码</span>
              <KtInput
                type="password"
                value={value.password}
                onChange={(event) =>
                  onChange(updateAccountFormField(value, 'password', event.target.value))
                }
              />
            </div>
          </div>
          <span style={accountFieldHintStyle}>
            开启后，登录窗口、浏览器任务和插件 HTTP 请求都会使用这个代理。
          </span>
        </div>
      )}
    </div>
  )
}

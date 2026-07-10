import { KtInput, KtSelect } from '@/shared/ui'
import type { BrowserOs, BrowserProfileFormState } from '../utils/browser-profile'
import {
  accountFieldHintStyle,
  accountFieldLabelStyle,
  accountFieldStyle,
  accountFormStackStyle,
  accountTwoColumnGridStyle,
  updateAccountFormField,
} from './account-form-shared'
import AccountSettingSwitch from './AccountSettingSwitch'

interface AccountBrowserProfileFieldsProps {
  value: BrowserProfileFormState
  onChange: (value: BrowserProfileFormState) => void
}

const osOptions: { value: BrowserOs; label: string }[] = [
  { value: 'windows', label: 'Windows 10/11' },
  { value: 'macos', label: 'macOS' },
  { value: 'linux', label: 'Linux' },
]

const browserProfileGridStyle = accountTwoColumnGridStyle(140)

export default function AccountBrowserProfileFields({
  value,
  onChange,
}: AccountBrowserProfileFieldsProps) {
  return (
    <div style={accountFormStackStyle}>
      <AccountSettingSwitch
        checked={value.enabled}
        onChange={(enabled) => onChange(updateAccountFormField(value, 'enabled', enabled))}
      >
        模拟这个账号的系统和浏览器版本
      </AccountSettingSwitch>

      {value.enabled && (
        <div style={accountFormStackStyle}>
          <div style={browserProfileGridStyle}>
            <div style={accountFieldStyle}>
              <span style={accountFieldLabelStyle}>系统</span>
              <KtSelect
                value={value.os}
                onChange={(os) => onChange(updateAccountFormField(value, 'os', os as BrowserOs))}
                options={osOptions}
              />
            </div>
            <div style={accountFieldStyle}>
              <span style={accountFieldLabelStyle}>Chrome 主版本</span>
              <KtInput
                inputMode="numeric"
                placeholder="126"
                value={value.browserVersion}
                onChange={(event) =>
                  onChange(updateAccountFormField(value, 'browserVersion', event.target.value))
                }
              />
            </div>
          </div>
          <span style={accountFieldHintStyle}>
            开启后，网站看到的系统和 Chrome 版本会按这里发送；不确定时保持关闭。
          </span>
        </div>
      )}
    </div>
  )
}

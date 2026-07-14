import { KtSelect } from '@/shared/ui'
import {
  getDefaultChromeMajorVersion,
  type BrowserOs,
  type BrowserProfileFormState,
} from '../utils/browser-profile'
import {
  accountFieldHintStyle,
  accountFieldLabelStyle,
  accountFieldStyle,
  accountFormStackStyle,
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

export default function AccountBrowserProfileFields({
  value,
  onChange,
}: AccountBrowserProfileFieldsProps) {
  const chromeMajorVersion = getDefaultChromeMajorVersion(value.os)

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
          <div style={accountFieldStyle}>
            <span style={accountFieldLabelStyle}>系统</span>
            <KtSelect
              value={value.os}
              onChange={(os) => onChange(updateAccountFormField(value, 'os', os as BrowserOs))}
              options={osOptions}
            />
          </div>
          <span style={accountFieldHintStyle}>
            Chrome 版本会自动使用所选系统当前的稳定主版本 {chromeMajorVersion}，无需手动填写。
          </span>
        </div>
      )}
    </div>
  )
}

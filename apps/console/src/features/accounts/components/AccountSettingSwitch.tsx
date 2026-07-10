import type { ReactNode } from 'react'
import { accountFieldLabelStyle } from './account-form-shared'

interface AccountSettingSwitchProps {
  checked: boolean
  onChange: (checked: boolean) => void
  children: ReactNode
}

// 账号配置弹窗里的开关统一走这里，避免每个设置项重复写 checkbox 结构。
export default function AccountSettingSwitch({
  checked,
  onChange,
  children,
}: AccountSettingSwitchProps) {
  return (
    <label style={accountFieldLabelStyle}>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
      {children}
    </label>
  )
}

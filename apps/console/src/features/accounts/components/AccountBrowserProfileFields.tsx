import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { Apple, PanelsTopLeft } from 'lucide-react'
import { fetchBrowserFingerprintOptions } from '@/shared/services/browser-session-service'
import {
  browserVersionChoices,
  type BrowserOs,
  type BrowserProfileFormState,
} from '../utils/browser-profile'
import { accountFieldHintStyle, accountFormStackStyle } from './account-form-shared'

interface AccountBrowserProfileFieldsProps {
  value: BrowserProfileFormState
  onChange: (value: BrowserProfileFormState) => void
  disabled?: boolean
  onInitialized?: (value: BrowserProfileFormState) => void
}

const osOptions: { value: BrowserOs; label: string; icon: ReactNode }[] = [
  { value: 'windows', label: 'Windows', icon: <PanelsTopLeft size={15} /> },
  { value: 'macos', label: 'macOS', icon: <Apple size={15} /> },
  { value: 'linux', label: 'Linux', icon: '🐧' },
]

const rowStyle: CSSProperties = { display: 'flex', flexWrap: 'wrap', gap: 8 }
const optionStyle: CSSProperties = {
  border: '1px solid var(--border-muted)',
  borderRadius: 8,
  background: 'var(--bg-card)',
  color: 'var(--text-secondary)',
  padding: '8px 12px',
  cursor: 'pointer',
  fontSize: 13,
  minWidth: 88,
}

export default function AccountBrowserProfileFields({
  value,
  onChange,
  disabled = false,
  onInitialized,
}: AccountBrowserProfileFieldsProps) {
  const [latestVersion, setLatestVersion] = useState('')
  const [installedVersion, setInstalledVersion] = useState('')
  const [loadError, setLoadError] = useState(false)
  const latest = useRef({ value, onChange, onInitialized })
  latest.current = { value, onChange, onInitialized }

  useEffect(() => {
    let active = true
    void fetchBrowserFingerprintOptions()
      .then(({ browser_version, installed_browser_version, os }) => {
        if (active) {
          setLatestVersion(browser_version)
          setInstalledVersion(installed_browser_version)
          setLoadError(false)
          if (!latest.current.value.version) {
            const initial = {
              ...latest.current.value,
              enabled: true,
              os: osOptions.some((option) => option.value === os) ? os : 'windows',
              version: browser_version,
            } satisfies BrowserProfileFormState
            const initialize = latest.current.onInitialized ?? latest.current.onChange
            initialize(initial)
          }
        }
      })
      .catch(() => {
        if (active) setLoadError(true)
      })
    return () => {
      active = false
    }
  }, [])

  const versions = browserVersionChoices(latestVersion)
  if (value.version && !versions.includes(value.version)) versions.push(value.version)
  const selectedVersion = value.version || latestVersion
  const chooseOs = (os: BrowserOs) =>
    onChange({ ...value, enabled: true, os, version: selectedVersion })
  const chooseVersion = (version: string) => onChange({ ...value, enabled: true, version })
  const option = (label: string, selected: boolean, onClick: () => void, icon?: ReactNode) => (
    <button
      type="button"
      disabled={disabled}
      aria-pressed={selected}
      onClick={onClick}
      style={{
        ...optionStyle,
        borderColor: selected ? 'var(--color-brand)' : 'var(--border-muted)',
        background: selected ? 'var(--color-brand-bg)' : 'var(--bg-card)',
        color: selected ? 'var(--text-primary)' : 'var(--text-secondary)',
        cursor: disabled ? 'not-allowed' : 'pointer',
      }}
    >
      {icon && (
        <span
          data-system-icon
          aria-hidden="true"
          style={{ marginRight: 6, verticalAlign: 'middle' }}
        >
          {icon}
        </span>
      )}
      {label}
    </button>
  )

  return (
    <div style={accountFormStackStyle}>
      <strong style={{ color: 'var(--text-primary)', fontSize: 14 }}>系统指纹</strong>
      <div style={rowStyle} aria-label="系统指纹">
        {osOptions.map(({ value: os, label, icon }) => (
          <span key={os}>
            {option(label, value.enabled && value.os === os, () => chooseOs(os), icon)}
          </span>
        ))}
      </div>
      <span style={{ color: 'var(--text-secondary)', fontSize: 13 }}>浏览器版本</span>
      <div style={rowStyle} aria-label="浏览器版本">
        {versions.map((version) => (
          <span key={version}>
            {option(
              `Chrome ${version}${version === latestVersion ? ' · 最新稳定版' : ''}`,
              value.enabled && selectedVersion === version,
              () => chooseVersion(version)
            )}
          </span>
        ))}
        {loadError && <span style={accountFieldHintStyle}>无法读取浏览器版本，请稍后重试。</span>}
      </div>
      <span style={accountFieldHintStyle}>
        系统和版本会用于浏览器 UA、客户端提示及插件请求。当前浏览器内核为 Chrome{' '}
        {installedVersion || '未知'}；版本模拟不会升级内核、字体等底层特征。
      </span>
    </div>
  )
}

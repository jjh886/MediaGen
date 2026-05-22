import { KtInput } from '@/shared/ui'
import type { AppSettings } from '@/shared/services/settings-service'
import { cardStyle, titleStyle, Field, strHandler } from './settings-shared'

const radioLabelStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  cursor: 'pointer',
  fontSize: 13,
}

const radioStyle: React.CSSProperties = { accentColor: '#5AB0FE' }

function ModeRadioGroup({
  value,
  onChange,
  randomLabel = '每次随机生成',
  fixedLabel = '固定',
}: {
  value: 'random' | 'default'
  onChange: (v: 'random' | 'default') => void
  randomLabel?: string
  fixedLabel?: string
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 6 }}>
      <label style={radioLabelStyle}>
        <input
          type="radio"
          checked={value === 'random'}
          onChange={() => onChange('random')}
          style={radioStyle}
        />
        {randomLabel}
      </label>
      <label style={radioLabelStyle}>
        <input
          type="radio"
          checked={value === 'default'}
          onChange={() => onChange('default')}
          style={radioStyle}
        />
        {fixedLabel}
      </label>
    </div>
  )
}

export function AutoRegisterCard({
  settings: s,
  update,
}: {
  settings: AppSettings
  update: <K extends keyof AppSettings>(k: K, v: AppSettings[K]) => void
}) {
  return (
    <div style={cardStyle}>
      <h2 style={titleStyle}>自动注册</h2>
      <div style={{ fontSize: 13, color: 'var(--text-secondary)' }}>
        所有账号配额耗尽时，自动注册新账号继续任务（仅限支持注册的平台）
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <label
          style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', fontSize: 14 }}
        >
          <input
            type="checkbox"
            checked={s.auto_register_enabled}
            onChange={(e) => update('auto_register_enabled', e.target.checked)}
            style={{ width: 18, height: 18, accentColor: '#5AB0FE' }}
          />
          启用自动注册
        </label>
      </div>
      <Field label="注册密码">
        <ModeRadioGroup
          value={s.auto_register_password_mode}
          onChange={(v) => update('auto_register_password_mode', v)}
          fixedLabel="固定密码"
        />
        {s.auto_register_password_mode === 'default' && (
          <KtInput
            value={s.auto_register_password}
            onChange={strHandler(update, 'auto_register_password')}
          />
        )}
      </Field>
      <Field label="生日（需成年）">
        <ModeRadioGroup
          value={s.auto_register_birthday_mode}
          onChange={(v) => update('auto_register_birthday_mode', v)}
          fixedLabel="固定生日"
        />
        {s.auto_register_birthday_mode === 'default' && (
          <KtInput
            value={s.auto_register_birthday}
            onChange={strHandler(update, 'auto_register_birthday')}
            placeholder="1995-06-15"
          />
        )}
      </Field>
    </div>
  )
}

import { type CSSProperties, type SelectHTMLAttributes } from 'react'

interface KtSelectOption {
  value: string
  label: string
}

interface KtSelectProps extends Omit<
  SelectHTMLAttributes<HTMLSelectElement>,
  'value' | 'onChange'
> {
  value: string
  onChange: (value: string) => void
  options: KtSelectOption[]
  placeholder?: string
}

const selectStyle: CSSProperties = {
  height: 36,
  width: '100%',
  border: '1px solid var(--border-light)',
  borderRadius: 8,
  background: 'var(--input-bg)',
  color: 'var(--text-primary)',
  padding: '0 32px 0 12px',
  fontSize: 14,
}

function KtSelect({ value, onChange, options, placeholder, style, ...rest }: KtSelectProps) {
  const normalizedOptions = placeholder ? [{ value: '', label: placeholder }, ...options] : options

  return (
    <select
      {...rest}
      value={value}
      onChange={(event) => onChange(event.target.value)}
      style={{ ...selectStyle, ...style }}
    >
      {normalizedOptions.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  )
}

KtSelect.displayName = 'KtSelect'

export { KtSelect }
export type { KtSelectProps, KtSelectOption }

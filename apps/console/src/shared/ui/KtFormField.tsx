import type { CSSProperties, ReactNode } from 'react'

export const ktFormFieldStyle: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 6,
}

export const ktFormLabelStyle: CSSProperties = {
  color: 'var(--text-secondary)',
  fontSize: 13,
}

const errorStyle: CSSProperties = {
  color: 'var(--color-error)',
  fontSize: 12,
  lineHeight: '18px',
}

export function KtFormField({
  label,
  error,
  children,
}: {
  label: string
  error?: string
  children: ReactNode
}) {
  return (
    <label style={ktFormFieldStyle}>
      <span style={ktFormLabelStyle}>{label}</span>
      {children}
      {error && <span style={errorStyle}>{error}</span>}
    </label>
  )
}

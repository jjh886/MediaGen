import type { CSSProperties } from 'react'
import { ktFormFieldStyle, ktFormLabelStyle } from '@/shared/ui/KtFormField'

export const accountFormStackStyle: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 12,
}

export const accountFieldLabelStyle: CSSProperties = {
  ...ktFormLabelStyle,
  display: 'flex',
  alignItems: 'center',
  gap: 8,
}

export const accountFieldStyle: CSSProperties = ktFormFieldStyle

export const accountFieldHintStyle: CSSProperties = {
  color: 'var(--text-disabled)',
  fontSize: 12,
}

export function accountTwoColumnGridStyle(sideWidth: number): CSSProperties {
  return {
    display: 'grid',
    gridTemplateColumns: `1fr ${sideWidth}px`,
    gap: 10,
  }
}

export function updateAccountFormField<T, K extends keyof T>(value: T, key: K, next: T[K]): T {
  return { ...value, [key]: next }
}

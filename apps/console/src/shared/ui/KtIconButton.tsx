import { type ButtonHTMLAttributes, type ReactNode } from 'react'
import { KtButton } from './KtButton'

interface KtIconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  ariaLabel: string
  children: ReactNode
  title?: string
  variant?: 'primary' | 'default' | 'text' | 'danger'
}

export function KtIconButton({
  ariaLabel,
  children,
  style,
  title,
  variant = 'default',
  ...props
}: KtIconButtonProps) {
  return (
    <KtButton
      {...props}
      aria-label={ariaLabel}
      size="sm"
      title={title || ariaLabel}
      variant={variant}
      style={{ width: 32, padding: 0, ...style }}
    >
      {children}
    </KtButton>
  )
}

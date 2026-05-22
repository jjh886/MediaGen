import React from 'react'

interface KtBadgeProps {
  variant?: 'success' | 'warning' | 'error' | 'info' | 'disabled'
  children: React.ReactNode
}

const variantStyles: Record<
  NonNullable<KtBadgeProps['variant']>,
  { background: string; color: string }
> = {
  success: { background: 'rgba(82,196,26,0.1)', color: '#52C41A' },
  warning: { background: 'rgba(255,162,54,0.1)', color: '#FFA236' },
  error: { background: 'rgba(255,77,79,0.1)', color: '#FF4D4F' },
  info: { background: 'rgba(90,176,254,0.1)', color: '#5AB0FE' },
  disabled: { background: 'rgba(255,255,255,0.05)', color: 'rgba(255,255,255,0.45)' },
}

const baseStyle: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  height: 22,
  fontSize: 12,
  lineHeight: '22px',
  padding: '0 8px',
  borderRadius: 4,
}

function KtBadge({ variant = 'info', children }: KtBadgeProps) {
  const variantStyle = variantStyles[variant]

  return <span style={{ ...baseStyle, ...variantStyle }}>{children}</span>
}

export type { KtBadgeProps }
export { KtBadge }

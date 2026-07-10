import { type CSSProperties, type ButtonHTMLAttributes } from 'react'

interface KtButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'default' | 'text' | 'danger'
  size?: 'sm' | 'md' | 'lg'
  loading?: boolean
}

const variantStyles: Record<string, CSSProperties> = {
  primary: {
    background: 'linear-gradient(135deg, #2C35FA, #5BB4FE)',
    color: '#FFFFFF',
    border: 'none',
  },
  default: {
    background: '#2C2B2C',
    color: 'rgba(255,255,255,0.8)',
    border: '1px solid rgba(255,255,255,0.05)',
  },
  text: {
    background: 'transparent',
    color: '#5AB0FE',
    border: 'none',
  },
  danger: {
    background: '#FF4D4F',
    color: '#FFFFFF',
    border: 'none',
  },
}

const sizeStyles: Record<string, CSSProperties> = {
  lg: { height: 40, fontSize: 16, padding: '0 24px' },
  md: { height: 36, fontSize: 14, padding: '0 16px' },
  sm: { height: 28, fontSize: 12, padding: '0 12px' },
}

const baseStyle: CSSProperties = {
  borderRadius: 8,
  cursor: 'pointer',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 6,
  transition: 'opacity 200ms ease',
  fontFamily: 'inherit',
  lineHeight: 1,
  boxSizing: 'border-box',
}

export function KtButton({
  variant = 'default',
  size = 'md',
  loading = false,
  disabled,
  style,
  children,
  onMouseEnter,
  onMouseLeave,
  ...rest
}: KtButtonProps) {
  const isDisabled = disabled || loading

  const mergedStyle: CSSProperties = {
    ...baseStyle,
    ...variantStyles[variant],
    ...sizeStyles[size],
    ...(isDisabled ? { opacity: 0.4, cursor: 'not-allowed' } : {}),
    ...style,
  }

  return (
    <button
      {...rest}
      disabled={isDisabled}
      style={mergedStyle}
      onMouseEnter={(e) => {
        if (!isDisabled) {
          e.currentTarget.style.opacity = '0.8'
        }
        onMouseEnter?.(e)
      }}
      onMouseLeave={(e) => {
        if (!isDisabled) {
          e.currentTarget.style.opacity = '1'
        }
        onMouseLeave?.(e)
      }}
    >
      {loading && <span aria-label="loading">⏳</span>}
      {children}
    </button>
  )
}

import React from 'react'

interface KtInputProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'size'> {
  size?: 'md' | 'lg'
  error?: boolean
}

const KtInput = React.forwardRef<HTMLInputElement, KtInputProps>(
  ({ size = 'md', error = false, style, onFocus, onBlur, ...rest }, ref) => {
    const [focused, setFocused] = React.useState(false)

    const height = size === 'lg' ? 40 : 36

    const baseStyle: React.CSSProperties = {
      height,
      backgroundColor: '#1E1F21',
      border: `1px solid ${error ? '#FF4D4F' : focused ? '#5AB0FE' : 'rgba(255,255,255,0.05)'}`,
      borderRadius: 8,
      color: 'rgba(255,255,255,0.8)',
      padding: '0 12px',
      fontSize: 14,
      outline: 'none',
      transition: 'border-color 200ms ease',
      boxSizing: 'border-box',
      width: '100%',
      ...style,
    }

    return (
      <input
        ref={ref}
        style={baseStyle}
        onFocus={(e) => {
          setFocused(true)
          onFocus?.(e)
        }}
        onBlur={(e) => {
          setFocused(false)
          onBlur?.(e)
        }}
        {...rest}
      />
    )
  }
)

KtInput.displayName = 'KtInput'

export { KtInput }
export type { KtInputProps }

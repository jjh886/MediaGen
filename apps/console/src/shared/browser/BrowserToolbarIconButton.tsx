import type { CSSProperties, ReactNode } from 'react'

interface BrowserToolbarIconButtonProps {
  label: string
  onClick: () => void
  children: ReactNode
}

// 远程浏览器弹窗顶部统一使用这个小按钮，避免登录和接管页面各自复制样式。
export default function BrowserToolbarIconButton({
  label,
  onClick,
  children,
}: BrowserToolbarIconButtonProps) {
  return (
    <button type="button" title={label} aria-label={label} onClick={onClick} style={buttonStyle}>
      {children}
    </button>
  )
}

const buttonStyle: CSSProperties = {
  width: 30,
  height: 30,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  border: '1px solid rgba(255,255,255,0.12)',
  borderRadius: 6,
  background: 'rgba(255,255,255,0.06)',
  color: '#fff',
  cursor: 'pointer',
  padding: 0,
}

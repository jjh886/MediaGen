import { useEffect, useRef, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

interface KtModalProps {
  open: boolean
  onClose: () => void
  title: string
  children: ReactNode
  footer?: ReactNode
  width?: number
  closeOnOverlayClick?: boolean
}

function ModalHeader({ title, onClose }: { title: string; onClose: () => void }) {
  return (
    <div
      style={{
        height: 56,
        background: '#1E1F21',
        borderBottom: '1px solid rgba(255,255,255,0.05)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '0 24px',
      }}
    >
      <span style={{ fontWeight: 600, fontSize: 16, lineHeight: '24px', color: '#fff' }}>
        {title}
      </span>
      <button
        onClick={onClose}
        style={{
          background: 'none',
          border: 'none',
          color: '#fff',
          fontSize: 24,
          cursor: 'pointer',
          padding: 0,
          lineHeight: 1,
        }}
        onMouseEnter={(e) => {
          e.currentTarget.style.opacity = '0.8'
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.opacity = '1'
        }}
        aria-label="关闭"
      >
        ×
      </button>
    </div>
  )
}

function ModalFooter({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        height: 64,
        background: '#1E1F21',
        borderTop: '1px solid rgba(255,255,255,0.05)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'flex-end',
        gap: 8,
        padding: '0 24px',
      }}
    >
      {children}
    </div>
  )
}

// useModalEscape 监听 Esc 关闭弹窗；open=false 时不挂载监听
function useModalEscape(open: boolean, onClose: () => void) {
  useEffect(() => {
    if (!open) return
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', handleKey)
    return () => document.removeEventListener('keydown', handleKey)
  }, [open, onClose])
}

// useBodyScrollLock 弹窗打开期间锁住 body 滚动，关闭时还原
function useBodyScrollLock(open: boolean) {
  useEffect(() => {
    if (!open) return
    const prev = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.body.style.overflow = prev
    }
  }, [open])
}

// ModalPanel 弹窗内容面板（头 + 主体 + 可选 footer）
function ModalPanel({
  title,
  onClose,
  children,
  footer,
  width,
}: Omit<KtModalProps, 'open'> & { width: number }) {
  return (
    <div
      style={{
        width,
        maxWidth: '90vw',
        borderRadius: 16,
        overflow: 'hidden',
        boxShadow: '0 4px 24px rgba(0,0,0,0.5)',
        animation: 'kt-modal-in 300ms ease-out',
      }}
    >
      <ModalHeader title={title} onClose={onClose} />
      <div
        style={{
          background: '#141414',
          maxHeight: '60vh',
          overflowY: 'auto',
          padding: 24,
        }}
      >
        {children}
      </div>
      {footer && <ModalFooter>{footer}</ModalFooter>}
    </div>
  )
}

export function KtModal({
  open,
  onClose,
  title,
  children,
  footer,
  width = 480,
  closeOnOverlayClick = true,
}: KtModalProps) {
  const overlayRef = useRef<HTMLDivElement>(null)
  useModalEscape(open, onClose)
  useBodyScrollLock(open)

  if (!open) return null

  const handleOverlayClick = (e: React.MouseEvent) => {
    if (closeOnOverlayClick && e.target === overlayRef.current) onClose()
  }

  return createPortal(
    <div
      ref={overlayRef}
      onClick={handleOverlayClick}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 9999,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: 'rgba(0,0,0,0.6)',
      }}
    >
      <ModalPanel title={title} onClose={onClose} footer={footer} width={width}>
        {children}
      </ModalPanel>
      <style>{`
        @keyframes kt-modal-in {
          from { opacity: 0; transform: scale(0.9); }
          to { opacity: 1; transform: scale(1); }
        }
      `}</style>
    </div>,
    document.body
  )
}

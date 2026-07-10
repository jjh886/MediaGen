import {
  useEffect,
  useId,
  useRef,
  type CSSProperties,
  type MouseEvent,
  type ReactNode,
  type RefObject,
} from 'react'
import { createPortal } from 'react-dom'

interface KtModalProps {
  open: boolean
  onClose: () => void
  title: string
  children: ReactNode
  footer?: ReactNode
  titleExtra?: ReactNode
  width?: number | string
  height?: number | string
  bodyStyle?: CSSProperties
  resizable?: boolean
  closeOnOverlayClick?: boolean
}

const headerStyle: CSSProperties = {
  height: 56,
  background: '#1E1F21',
  borderBottom: '1px solid rgba(255,255,255,0.05)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  padding: '0 24px',
}

const titleRowStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  minWidth: 0,
}

const titleStyle: CSSProperties = {
  fontWeight: 600,
  fontSize: 16,
  lineHeight: '24px',
  color: '#fff',
}

const closeButtonStyle: CSSProperties = {
  background: 'none',
  border: 'none',
  color: '#fff',
  fontSize: 24,
  cursor: 'pointer',
  padding: 0,
  lineHeight: 1,
}

const footerStyle: CSSProperties = {
  height: 64,
  background: '#1E1F21',
  borderTop: '1px solid rgba(255,255,255,0.05)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-end',
  gap: 8,
  padding: '0 24px',
}

const overlayStyle: CSSProperties = {
  position: 'fixed',
  inset: 0,
  zIndex: 9999,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  background: 'rgba(0,0,0,0.6)',
}

function panelStyle(
  width: number | string,
  height: number | string | undefined,
  resizable = false
) {
  return {
    width,
    height,
    maxWidth: '96vw',
    maxHeight: '92vh',
    borderRadius: 16,
    overflow: 'hidden',
    boxShadow: '0 4px 24px rgba(0,0,0,0.5)',
    animation: 'kt-modal-in 300ms ease-out',
    display: 'flex',
    flexDirection: 'column',
    resize: resizable ? 'both' : undefined,
    minWidth: resizable ? 'min(720px, calc(100vw - 32px))' : undefined,
    minHeight: resizable ? 'min(520px, calc(100vh - 32px))' : undefined,
  } satisfies CSSProperties
}

function modalBodyStyle(height: number | string | undefined, bodyStyle?: CSSProperties) {
  return {
    background: '#141414',
    flex: height ? 1 : undefined,
    minHeight: 0,
    maxHeight: height ? undefined : '60vh',
    overflowY: 'auto',
    padding: 24,
    ...bodyStyle,
  } satisfies CSSProperties
}

function ModalHeader({
  title,
  titleId,
  titleExtra,
  onClose,
}: {
  title: string
  titleId: string
  titleExtra?: ReactNode
  onClose: () => void
}) {
  return (
    <div style={headerStyle}>
      <div style={titleRowStyle}>
        <span id={titleId} style={titleStyle}>
          {title}
        </span>
        {titleExtra}
      </div>
      <button
        onClick={onClose}
        style={closeButtonStyle}
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
  return <div style={footerStyle}>{children}</div>
}

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

function useModalFocus(open: boolean, panelRef: RefObject<HTMLDivElement | null>) {
  useEffect(() => {
    if (!open) return
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    window.setTimeout(() => panelRef.current?.focus(), 0)
    return () => previous?.focus()
  }, [open, panelRef])
}

function ModalPanel({
  title,
  titleId,
  onClose,
  children,
  footer,
  titleExtra,
  width,
  height,
  bodyStyle,
  resizable,
  panelRef,
}: Omit<KtModalProps, 'open'> & {
  width: number | string
  titleId: string
  panelRef: RefObject<HTMLDivElement | null>
}) {
  return (
    <div
      ref={panelRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      tabIndex={-1}
      style={panelStyle(width, height, resizable)}
    >
      <ModalHeader title={title} titleId={titleId} titleExtra={titleExtra} onClose={onClose} />
      <div style={modalBodyStyle(height, bodyStyle)}>{children}</div>
      {footer && <ModalFooter>{footer}</ModalFooter>}
    </div>
  )
}

function ModalOverlay({
  overlayRef,
  onOverlayClick,
  children,
}: {
  overlayRef: RefObject<HTMLDivElement | null>
  onOverlayClick: (event: MouseEvent) => void
  children: ReactNode
}) {
  return (
    <div ref={overlayRef} onClick={onOverlayClick} style={overlayStyle}>
      {children}
      <style>{`
        @keyframes kt-modal-in {
          from { opacity: 0; transform: scale(0.9); }
          to { opacity: 1; transform: scale(1); }
        }
      `}</style>
    </div>
  )
}

export function KtModal({
  open,
  onClose,
  title,
  children,
  footer,
  titleExtra,
  width = 480,
  height,
  bodyStyle,
  resizable = false,
  closeOnOverlayClick = true,
}: KtModalProps) {
  const overlayRef = useRef<HTMLDivElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const titleId = useId()
  useModalEscape(open, onClose)
  useBodyScrollLock(open)
  useModalFocus(open, panelRef)

  if (!open) return null

  const handleOverlayClick = (e: MouseEvent) => {
    if (closeOnOverlayClick && e.target === overlayRef.current) onClose()
  }

  return createPortal(
    <ModalOverlay overlayRef={overlayRef} onOverlayClick={handleOverlayClick}>
      <ModalPanel
        title={title}
        titleId={titleId}
        onClose={onClose}
        footer={footer}
        titleExtra={titleExtra}
        width={width}
        height={height}
        bodyStyle={bodyStyle}
        resizable={resizable}
        panelRef={panelRef}
      >
        {children}
      </ModalPanel>
    </ModalOverlay>,
    document.body
  )
}

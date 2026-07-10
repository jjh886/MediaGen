import { KtButton } from './KtButton'
import { KtModal } from './KtModal'

interface KtConfirmDialogProps {
  open: boolean
  title: string
  message: string
  confirmText?: string
  cancelText?: string
  danger?: boolean
  loading?: boolean
  onCancel: () => void
  onConfirm: () => void
}

export function KtConfirmDialog({
  open,
  title,
  message,
  confirmText = '确认',
  cancelText = '取消',
  danger = false,
  loading = false,
  onCancel,
  onConfirm,
}: KtConfirmDialogProps) {
  return (
    <KtModal
      open={open}
      onClose={onCancel}
      title={title}
      closeOnOverlayClick={false}
      footer={
        <>
          <KtButton variant="default" disabled={loading} onClick={onCancel}>
            {cancelText}
          </KtButton>
          <KtButton variant={danger ? 'danger' : 'primary'} loading={loading} onClick={onConfirm}>
            {confirmText}
          </KtButton>
        </>
      }
    >
      <div style={{ color: 'var(--text-secondary)', fontSize: 14, lineHeight: '22px' }}>
        {message}
      </div>
    </KtModal>
  )
}

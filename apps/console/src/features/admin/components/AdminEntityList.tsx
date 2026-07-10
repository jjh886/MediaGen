import { useState, type CSSProperties, type ReactNode } from 'react'
import { Trash2 } from 'lucide-react'
import { KtAsyncState, KtConfirmDialog, KtIconButton } from '@/shared/ui'

const entityCardStyle: CSSProperties = {
  background: 'var(--bg-card)',
  border: '1px solid var(--border-base)',
  borderRadius: 8,
  padding: 16,
}

const entityRowStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: '1.2fr 1.5fr 1fr auto',
  gap: 12,
  alignItems: 'center',
  padding: '10px 0',
  borderBottom: '1px solid var(--border-light)',
}

interface EntityListProps<T> {
  items: T[]
  loading: boolean
  error: string
  emptyText: string
  columns: (item: T) => string[]
  onRetry: () => void
  onDelete?: (item: T) => Promise<void>
  deleteTitle?: string
  deleteMessage?: (item: T) => string
  renderActions?: (item: T) => ReactNode
  rowKey?: (item: T, index: number) => string
}

function useDeleteAction<T>(onDelete?: (item: T) => Promise<void>) {
  const [deleting, setDeleting] = useState<T | null>(null)
  const [deleteError, setDeleteError] = useState('')

  const close = () => {
    setDeleting(null)
    setDeleteError('')
  }

  const confirm = async () => {
    if (!deleting || !onDelete) return
    try {
      await onDelete(deleting)
      close()
    } catch (error) {
      setDeleteError(error instanceof Error ? error.message : '删除失败，请重试。')
    }
  }

  return { close, confirm, deleteError, deleting, setDeleting }
}

function EntityRows<T>({
  columns,
  items,
  onDeleteClick,
  renderActions,
  rowKey,
}: Pick<EntityListProps<T>, 'columns' | 'items' | 'renderActions' | 'rowKey'> & {
  onDeleteClick?: (item: T) => void
}) {
  return items.map((item, index) => (
    <EntityRow<T>
      key={rowKey?.(item, index) ?? String(index)}
      columns={columns(item)}
      item={item}
      onDeleteClick={onDeleteClick}
      renderActions={renderActions}
    />
  ))
}

function EntityRow<T>({
  columns,
  item,
  onDeleteClick,
  renderActions,
}: {
  columns: string[]
  item: T
  onDeleteClick?: (item: T) => void
  renderActions?: (item: T) => ReactNode
}) {
  return (
    <div style={entityRowStyle}>
      {columns.slice(0, 3).map((value, colIndex) => (
        <span key={colIndex}>{value}</span>
      ))}
      <span style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
        {renderActions?.(item)}
        {onDeleteClick && (
          <KtIconButton ariaLabel="删除" onClick={() => onDeleteClick(item)}>
            <Trash2 size={16} />
          </KtIconButton>
        )}
      </span>
    </div>
  )
}

function DeleteError({ value }: { value: string }) {
  if (!value) return null
  return <div style={{ color: 'var(--color-error)', fontSize: 13 }}>{value}</div>
}

function DeleteConfirm<T>({
  action,
  deleteMessage,
  deleteTitle,
}: {
  action: ReturnType<typeof useDeleteAction<T>>
  deleteMessage?: (item: T) => string
  deleteTitle?: string
}) {
  const message = action.deleting && deleteMessage ? deleteMessage(action.deleting) : '确定删除吗？'
  return (
    <KtConfirmDialog
      open={Boolean(action.deleting)}
      title={deleteTitle || '确认删除'}
      message={message}
      confirmText="删除"
      danger
      onCancel={action.close}
      onConfirm={action.confirm}
    />
  )
}

export function AdminEntityList<T>(props: EntityListProps<T>) {
  const deleteAction = useDeleteAction(props.onDelete)
  const showRows = !props.loading && !props.error

  return (
    <div style={entityCardStyle}>
      <KtAsyncState
        loading={props.loading}
        error={props.error}
        empty={showRows && props.items.length === 0}
        emptyText={props.emptyText}
        onRetry={props.onRetry}
      />
      {showRows && (
        <EntityRows<T>
          items={props.items}
          columns={props.columns}
          rowKey={props.rowKey}
          renderActions={props.renderActions}
          onDeleteClick={props.onDelete ? deleteAction.setDeleting : undefined}
        />
      )}
      <DeleteError value={deleteAction.deleteError} />
      <DeleteConfirm<T>
        action={deleteAction}
        deleteTitle={props.deleteTitle}
        deleteMessage={props.deleteMessage}
      />
    </div>
  )
}

import type React from 'react'

export interface Column<T> {
  key: string
  title: string
  renderHeader?: () => React.ReactNode
  width?: number | string
  minWidth?: number
  maxWidth?: number
  resizable?: boolean
  sticky?: 'right'
  render?: (record: T, index: number) => React.ReactNode
}

export interface KtTableProps<T> {
  columns: Column<T>[]
  data: T[]
  rowKey: string | ((record: T) => string)
  onRowClick?: (record: T) => void
  selectedRowKey?: string
  emptyText?: string
  expandedRowRender?: (record: T, index: number) => React.ReactNode
  isRowExpanded?: (record: T) => boolean
}

export interface MeasuredColumn<T> extends Column<T> {
  measuredWidth: number
  minWidthValue: number
  maxWidthValue: number
}

const DEFAULT_COLUMN_WIDTH = 160
const DEFAULT_MIN_WIDTH = 72
const DEFAULT_MAX_WIDTH = 900
const TABLE_BODY_BACKGROUND = 'var(--bg-primary)'
const TABLE_HEADER_BACKGROUND = 'var(--bg-card)'
const TABLE_ROW_HOVER_BACKGROUND = 'rgb(22, 22, 25)'
const TABLE_ROW_SELECTED_BACKGROUND = 'rgb(19, 23, 30)'

export const tableStyles = {
  wrapper: {
    width: '100%',
    overflowX: 'auto' as const,
  },
  table: {
    width: '100%',
    borderCollapse: 'separate' as const,
    borderSpacing: 0,
    tableLayout: 'fixed' as const,
  },
  headerCell: {
    background: TABLE_HEADER_BACKGROUND,
    color: 'rgba(255,255,255,0.6)',
    fontSize: 13,
    fontWeight: 500 as const,
    padding: '0 var(--kt-table-cell-padding-x, 16px)',
    height: 40,
    textAlign: 'left' as const,
    borderBottom: '1px solid rgba(255,255,255,0.05)',
    boxSizing: 'border-box' as const,
    position: 'relative' as const,
  },
  row: {
    height: 48,
    background: 'transparent',
    borderBottom: '1px solid rgba(255,255,255,0.05)',
    transition: 'background 150ms ease',
  },
  cell: {
    padding: '0 var(--kt-table-cell-padding-x, 16px)',
    fontSize: 14,
    color: 'rgba(255,255,255,0.85)',
    overflow: 'hidden' as const,
    textOverflow: 'ellipsis' as const,
    whiteSpace: 'nowrap' as const,
    boxSizing: 'border-box' as const,
  },
  empty: {
    textAlign: 'center' as const,
    color: 'rgba(255,255,255,0.4)',
    padding: '32px 0',
    fontSize: 14,
  },
  resizeHandle: {
    position: 'absolute' as const,
    top: 0,
    right: 0,
    bottom: 0,
    width: 10,
    cursor: 'col-resize',
    zIndex: 1,
  },
  resizeHandleLine: {
    position: 'absolute' as const,
    top: 8,
    right: 4,
    bottom: 8,
    width: 1,
    background: 'rgba(90,176,254,0.2)',
  },
  expandedCell: {
    padding: 0,
    borderBottom: '1px solid rgba(255,255,255,0.05)',
  },
} as const

export function getRowKey<T>(record: T, rowKey: string | ((record: T) => string)): string {
  return typeof rowKey === 'function'
    ? rowKey(record)
    : String((record as Record<string, unknown>)[rowKey])
}

export function measureColumn<T>(
  column: Column<T>,
  columnWidths: Record<string, number>
): MeasuredColumn<T> {
  const minWidth = column.minWidth ?? DEFAULT_MIN_WIDTH
  const maxWidth = Math.max(column.maxWidth ?? DEFAULT_MAX_WIDTH, minWidth)
  const defaultWidth = widthToNumber(column.width, DEFAULT_COLUMN_WIDTH)
  const measuredWidth = clampWidth(columnWidths[column.key] ?? defaultWidth, minWidth, maxWidth)
  return { ...column, measuredWidth, minWidthValue: minWidth, maxWidthValue: maxWidth }
}

export function getCellText<T>(record: T, key: string): string {
  const value = (record as Record<string, unknown>)[key]
  return value == null ? '' : String(value)
}

export function getRowBackground(isSelected: boolean, isHovered: boolean): string {
  if (isSelected) return TABLE_ROW_SELECTED_BACKGROUND
  if (isHovered) return TABLE_ROW_HOVER_BACKGROUND
  return 'transparent'
}

export function getStickyColumnStyle<T>(column: MeasuredColumn<T>, background: string) {
  if (column.sticky !== 'right') return {}
  return {
    position: 'sticky' as const,
    right: 0,
    zIndex: 2,
    background,
    boxShadow: '-16px 0 18px -18px rgba(0,0,0,0.9)',
  }
}

export function getBodyCellBackground(rowBackground: string): string {
  return rowBackground === 'transparent' ? TABLE_BODY_BACKGROUND : rowBackground
}

export function startColumnResize<T>(
  event: React.MouseEvent,
  column: MeasuredColumn<T>,
  setColumnWidths: React.Dispatch<React.SetStateAction<Record<string, number>>>
) {
  event.preventDefault()
  event.stopPropagation()

  const startX = event.clientX
  const startWidth = column.measuredWidth
  const previousUserSelect = document.body.style.userSelect

  document.body.style.userSelect = 'none'

  const handleMouseMove = (moveEvent: MouseEvent) => {
    const nextWidth = clampWidth(
      startWidth + moveEvent.clientX - startX,
      column.minWidthValue,
      column.maxWidthValue
    )
    setColumnWidths((prev) => ({ ...prev, [column.key]: nextWidth }))
  }

  const handleMouseUp = () => {
    document.body.style.userSelect = previousUserSelect
    document.removeEventListener('mousemove', handleMouseMove)
    document.removeEventListener('mouseup', handleMouseUp)
  }

  document.addEventListener('mousemove', handleMouseMove)
  document.addEventListener('mouseup', handleMouseUp)
}

function widthToNumber(width: number | string | undefined, fallback: number): number {
  if (typeof width === 'number' && Number.isFinite(width)) return width
  if (typeof width !== 'string') return fallback

  const parsed = Number.parseInt(width, 10)
  return Number.isFinite(parsed) ? parsed : fallback
}

function clampWidth(width: number, minWidth: number, maxWidth: number): number {
  return Math.min(Math.max(width, minWidth), maxWidth)
}

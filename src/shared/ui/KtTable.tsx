import React, { useCallback, useMemo, useRef, useState } from 'react'
import { KtTableHorizontalScrollbar } from './KtTableHorizontalScrollbar'

interface Column<T> {
  key: string
  title: string
  width?: number | string
  minWidth?: number
  maxWidth?: number
  resizable?: boolean
  sticky?: 'right'
  render?: (record: T, index: number) => React.ReactNode
}

interface KtTableProps<T> {
  columns: Column<T>[]
  data: T[]
  rowKey: string | ((record: T) => string)
  onRowClick?: (record: T) => void
  selectedRowKey?: string
  emptyText?: string
  expandedRowRender?: (record: T, index: number) => React.ReactNode
  isRowExpanded?: (record: T) => boolean
}

interface MeasuredColumn<T> extends Column<T> {
  measuredWidth: number
  minWidthValue: number
  maxWidthValue: number
}

const DEFAULT_COLUMN_WIDTH = 160
const DEFAULT_MIN_WIDTH = 72
const DEFAULT_MAX_WIDTH = 900

const styles = {
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
    background: 'rgba(31,31,31,0.5)',
    color: 'rgba(255,255,255,0.6)',
    fontSize: 13,
    fontWeight: 500 as const,
    padding: '0 16px',
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
    padding: '0 16px',
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

function getRowKey<T>(record: T, rowKey: string | ((record: T) => string)): string {
  return typeof rowKey === 'function'
    ? rowKey(record)
    : String((record as Record<string, unknown>)[rowKey])
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

function measureColumn<T>(
  column: Column<T>,
  columnWidths: Record<string, number>
): MeasuredColumn<T> {
  const minWidth = column.minWidth ?? DEFAULT_MIN_WIDTH
  const maxWidth = Math.max(column.maxWidth ?? DEFAULT_MAX_WIDTH, minWidth)
  const defaultWidth = widthToNumber(column.width, DEFAULT_COLUMN_WIDTH)
  const measuredWidth = clampWidth(columnWidths[column.key] ?? defaultWidth, minWidth, maxWidth)

  return {
    ...column,
    measuredWidth,
    minWidthValue: minWidth,
    maxWidthValue: maxWidth,
  }
}

function getCellText<T>(record: T, key: string): string {
  const value = (record as Record<string, unknown>)[key]
  return value == null ? '' : String(value)
}

function getRowBackground(isSelected: boolean, isHovered: boolean): string {
  if (isSelected) return 'rgba(90,176,254,0.05)'
  if (isHovered) return 'rgba(255,255,255,0.03)'
  return 'transparent'
}

function getStickyColumnStyle<T>(column: MeasuredColumn<T>, background: string) {
  if (column.sticky !== 'right') return {}

  return {
    position: 'sticky' as const,
    right: 0,
    zIndex: 2,
    background,
    boxShadow: '-16px 0 18px -18px rgba(0,0,0,0.9)',
  }
}

function getBodyCellBackground(rowBackground: string): string {
  return rowBackground === 'transparent' ? 'var(--bg-primary)' : rowBackground
}

function startColumnResize<T>(
  event: React.MouseEvent,
  column: MeasuredColumn<T>,
  setColumnWidths: React.Dispatch<React.SetStateAction<Record<string, number>>>
) {
  event.preventDefault()
  event.stopPropagation()

  const startX = event.clientX
  const startWidth = column.measuredWidth
  const previousUserSelect = document.body.style.userSelect

  // 拖动时禁用页面选中文本，避免列宽调整变成选中一整行文字。
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

interface HeaderCellProps<T> {
  column: MeasuredColumn<T>
  onResizeStart: (event: React.MouseEvent, column: MeasuredColumn<T>) => void
}

function HeaderCell<T>({ column, onResizeStart }: HeaderCellProps<T>) {
  return (
    <th
      style={{
        ...styles.headerCell,
        ...getStickyColumnStyle(column, styles.headerCell.background),
        ...(column.sticky === 'right' ? { zIndex: 3 } : {}),
        width: column.measuredWidth,
      }}
    >
      {column.title}
      {column.resizable !== false && (
        <span
          aria-label={`调整${column.title || '当前'}列宽`}
          role="separator"
          style={styles.resizeHandle}
          title="拖动调整列宽"
          onMouseDown={(event) => onResizeStart(event, column)}
          onClick={(event) => event.stopPropagation()}
        >
          <span style={styles.resizeHandleLine} />
        </span>
      )}
    </th>
  )
}

interface TableCellProps<T> {
  column: MeasuredColumn<T>
  rowBackground: string
  record: T
  index: number
}

function TableCell<T>({ column, rowBackground, record, index }: TableCellProps<T>) {
  const fallbackText = getCellText(record, column.key)
  const content = column.render ? column.render(record, index) : fallbackText

  return (
    <td
      style={{
        ...styles.cell,
        ...getStickyColumnStyle(column, getBodyCellBackground(rowBackground)),
        width: column.measuredWidth,
      }}
      title={typeof content === 'string' ? content : fallbackText}
    >
      {content}
    </td>
  )
}

interface BodyRowProps<T> {
  columns: MeasuredColumn<T>[]
  expandedRowRender?: (record: T, index: number) => React.ReactNode
  hoveredKey: string | null
  index: number
  isRowExpanded?: (record: T) => boolean
  onMouseEnter: (key: string) => void
  onMouseLeave: () => void
  onRowClick?: (record: T) => void
  record: T
  rowIdentity: string
  selectedRowKey?: string
}

function BodyRow<T>({
  columns,
  expandedRowRender,
  hoveredKey,
  index,
  isRowExpanded,
  onMouseEnter,
  onMouseLeave,
  onRowClick,
  record,
  rowIdentity,
  selectedRowKey,
}: BodyRowProps<T>) {
  const rowBg = getRowBackground(selectedRowKey === rowIdentity, hoveredKey === rowIdentity)
  const expanded = isRowExpanded?.(record) ?? false

  return (
    <>
      <tr
        style={{
          ...styles.row,
          background: rowBg,
          cursor: onRowClick ? 'pointer' : 'default',
        }}
        onClick={() => onRowClick?.(record)}
        onMouseEnter={() => onMouseEnter(rowIdentity)}
        onMouseLeave={onMouseLeave}
      >
        {columns.map((column) => (
          <TableCell<T>
            key={column.key}
            column={column}
            rowBackground={rowBg}
            record={record}
            index={index}
          />
        ))}
      </tr>
      {expanded && expandedRowRender && (
        <tr>
          <td colSpan={columns.length} style={styles.expandedCell}>
            {expandedRowRender(record, index)}
          </td>
        </tr>
      )}
    </>
  )
}

interface TableBodyProps<T> extends Pick<KtTableProps<T>, 'data' | 'emptyText' | 'rowKey'> {
  columns: MeasuredColumn<T>[]
  expandedRowRender?: (record: T, index: number) => React.ReactNode
  hoveredKey: string | null
  isRowExpanded?: (record: T) => boolean
  onMouseEnter: (key: string) => void
  onMouseLeave: () => void
  onRowClick?: (record: T) => void
  selectedRowKey?: string
}

function TableBody<T>({
  columns,
  data,
  emptyText,
  expandedRowRender,
  hoveredKey,
  isRowExpanded,
  onMouseEnter,
  onMouseLeave,
  onRowClick,
  rowKey,
  selectedRowKey,
}: TableBodyProps<T>) {
  if (data.length === 0) {
    return (
      <tbody>
        <tr>
          <td colSpan={columns.length} style={styles.empty}>
            {emptyText}
          </td>
        </tr>
      </tbody>
    )
  }

  return (
    <tbody>
      {data.map((record, index) => {
        const rowIdentity = getRowKey(record, rowKey)
        return (
          <BodyRow<T>
            key={rowIdentity}
            columns={columns}
            expandedRowRender={expandedRowRender}
            hoveredKey={hoveredKey}
            index={index}
            isRowExpanded={isRowExpanded}
            onMouseEnter={onMouseEnter}
            onMouseLeave={onMouseLeave}
            onRowClick={onRowClick}
            record={record}
            rowIdentity={rowIdentity}
            selectedRowKey={selectedRowKey}
          />
        )
      })}
    </tbody>
  )
}

function useMeasuredColumns<T>(columns: Column<T>[]) {
  const [columnWidths, setColumnWidths] = useState<Record<string, number>>({})
  const measuredColumns = useMemo(
    () => columns.map((column) => measureColumn(column, columnWidths)),
    [columns, columnWidths]
  )
  const tableMinWidth = useMemo(
    () => measuredColumns.reduce((total, column) => total + column.measuredWidth, 0),
    [measuredColumns]
  )
  const resizeColumn = useCallback((event: React.MouseEvent, column: MeasuredColumn<T>) => {
    startColumnResize(event, column, setColumnWidths)
  }, [])

  return { measuredColumns, resizeColumn, tableMinWidth }
}

function useTableHover() {
  const [hoveredKey, setHoveredKey] = useState<string | null>(null)
  const handleMouseEnter = useCallback((key: string) => {
    setHoveredKey(key)
  }, [])
  const handleMouseLeave = useCallback(() => {
    setHoveredKey(null)
  }, [])

  return { handleMouseEnter, handleMouseLeave, hoveredKey }
}

function ColumnGroup<T>({ columns }: { columns: MeasuredColumn<T>[] }) {
  return (
    <colgroup>
      {columns.map((col) => (
        <col key={col.key} style={{ width: col.measuredWidth }} />
      ))}
    </colgroup>
  )
}

function TableHeader<T>({
  columns,
  onResizeStart,
}: {
  columns: MeasuredColumn<T>[]
  onResizeStart: (event: React.MouseEvent, column: MeasuredColumn<T>) => void
}) {
  return (
    <thead>
      <tr>
        {columns.map((col) => (
          <HeaderCell<T> key={col.key} column={col} onResizeStart={onResizeStart} />
        ))}
      </tr>
    </thead>
  )
}

interface TableLayoutProps<T> extends KtTableProps<T> {
  handleMouseEnter: (key: string) => void
  handleMouseLeave: () => void
  hoveredKey: string | null
  measuredColumns: MeasuredColumn<T>[]
  onResizeStart: (event: React.MouseEvent, column: MeasuredColumn<T>) => void
  tableMinWidth: number
}

function TableLayout<T>(props: TableLayoutProps<T>) {
  const scrollRef = useRef<HTMLDivElement>(null)

  return (
    <>
      <div ref={scrollRef} className="kt-table__scroll" style={styles.wrapper}>
        <table style={{ ...styles.table, minWidth: props.tableMinWidth }}>
          <ColumnGroup<T> columns={props.measuredColumns} />
          <TableHeader<T> columns={props.measuredColumns} onResizeStart={props.onResizeStart} />
          <TableBody<T>
            columns={props.measuredColumns}
            data={props.data}
            emptyText={props.emptyText}
            expandedRowRender={props.expandedRowRender}
            hoveredKey={props.hoveredKey}
            isRowExpanded={props.isRowExpanded}
            onMouseEnter={props.handleMouseEnter}
            onMouseLeave={props.handleMouseLeave}
            onRowClick={props.onRowClick}
            rowKey={props.rowKey}
            selectedRowKey={props.selectedRowKey}
          />
        </table>
      </div>
      <KtTableHorizontalScrollbar scrollRef={scrollRef} />
    </>
  )
}

function KtTable<T>({ emptyText = '暂无数据', ...props }: KtTableProps<T>) {
  const { measuredColumns, resizeColumn, tableMinWidth } = useMeasuredColumns(props.columns)
  const hoverState = useTableHover()

  return (
    <TableLayout<T>
      {...props}
      {...hoverState}
      emptyText={emptyText}
      measuredColumns={measuredColumns}
      onResizeStart={resizeColumn}
      tableMinWidth={tableMinWidth}
    />
  )
}

export type { Column, KtTableProps }
export { KtTable }

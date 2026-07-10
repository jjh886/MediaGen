import type React from 'react'
import {
  getBodyCellBackground,
  getCellText,
  getRowBackground,
  getRowKey,
  getStickyColumnStyle,
  tableStyles,
  type KtTableProps,
  type MeasuredColumn,
} from './KtTableTypes'

interface HeaderCellProps<T> {
  column: MeasuredColumn<T>
  onResizeStart: (event: React.MouseEvent, column: MeasuredColumn<T>) => void
}

function HeaderCell<T>({ column, onResizeStart }: HeaderCellProps<T>) {
  return (
    <th
      style={{
        ...tableStyles.headerCell,
        ...getStickyColumnStyle(column, tableStyles.headerCell.background),
        ...(column.sticky === 'right' ? { zIndex: 3 } : {}),
        width: column.measuredWidth,
      }}
    >
      {column.title}
      {column.resizable !== false && (
        <span
          aria-label={`调整${column.title || '当前'}列宽`}
          role="separator"
          style={tableStyles.resizeHandle}
          title="拖动调整列宽"
          onMouseDown={(event) => onResizeStart(event, column)}
          onClick={(event) => event.stopPropagation()}
        >
          <span style={tableStyles.resizeHandleLine} />
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
        ...tableStyles.cell,
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

function BodyRow<T>(props: BodyRowProps<T>) {
  const rowBackground = getRowBackground(
    props.selectedRowKey === props.rowIdentity,
    props.hoveredKey === props.rowIdentity
  )
  const expanded = props.isRowExpanded?.(props.record) ?? false

  return (
    <>
      <DataRow<T> {...props} expanded={expanded} rowBackground={rowBackground} />
      <ExpandedRow<T> {...props} expanded={expanded} />
    </>
  )
}

function DataRow<T>({
  columns,
  expanded,
  expandedRowRender,
  index,
  onMouseEnter,
  onMouseLeave,
  onRowClick,
  record,
  rowBackground,
  rowIdentity,
}: BodyRowProps<T> & { expanded: boolean; rowBackground: string }) {
  const clickHandler = onRowClick ? () => onRowClick(record) : undefined
  return (
    <tr
      style={rowStyle(rowBackground, Boolean(onRowClick))}
      tabIndex={onRowClick ? 0 : undefined}
      aria-expanded={expandedRowRender ? expanded : undefined}
      onClick={clickHandler}
      onKeyDown={(event) => handleRowKeyDown(event, onRowClick, record)}
      onMouseEnter={() => onMouseEnter(rowIdentity)}
      onMouseLeave={onMouseLeave}
    >
      {columns.map((column) => (
        <TableCell<T>
          key={column.key}
          column={column}
          rowBackground={rowBackground}
          record={record}
          index={index}
        />
      ))}
    </tr>
  )
}

function ExpandedRow<T>({
  columns,
  expanded,
  expandedRowRender,
  index,
  record,
}: BodyRowProps<T> & { expanded: boolean }) {
  if (!expanded || !expandedRowRender) return null
  return (
    <tr>
      <td colSpan={columns.length} style={tableStyles.expandedCell}>
        {expandedRowRender(record, index)}
      </td>
    </tr>
  )
}

function handleRowKeyDown<T>(
  event: React.KeyboardEvent,
  onRowClick: ((record: T) => void) | undefined,
  record: T
) {
  if (!onRowClick || (event.key !== 'Enter' && event.key !== ' ')) return
  event.preventDefault()
  onRowClick(record)
}

function rowStyle(rowBackground: string, clickable: boolean) {
  return {
    ...tableStyles.row,
    background: rowBackground,
    cursor: clickable ? 'pointer' : 'default',
  }
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

export function TableBody<T>(props: TableBodyProps<T>) {
  if (props.data.length === 0) {
    return (
      <tbody>
        <tr>
          <td colSpan={props.columns.length} style={tableStyles.empty}>
            {props.emptyText}
          </td>
        </tr>
      </tbody>
    )
  }

  return (
    <tbody>
      {props.data.map((record, index) => (
        <BodyRow<T>
          key={getRowKey(record, props.rowKey)}
          {...props}
          index={index}
          record={record}
          rowIdentity={getRowKey(record, props.rowKey)}
        />
      ))}
    </tbody>
  )
}

export function ColumnGroup<T>({ columns }: { columns: MeasuredColumn<T>[] }) {
  return (
    <colgroup>
      {columns.map((col) => (
        <col key={col.key} style={{ width: col.measuredWidth }} />
      ))}
    </colgroup>
  )
}

export function TableHeader<T>({
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

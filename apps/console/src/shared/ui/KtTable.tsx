import React, { useCallback, useMemo, useRef, useState } from 'react'
import { KtTableHorizontalScrollbar } from './KtTableHorizontalScrollbar'
import { ColumnGroup, TableBody, TableHeader } from './KtTableSections'
import {
  measureColumn,
  startColumnResize,
  tableStyles,
  type Column,
  type KtTableProps,
  type MeasuredColumn,
} from './KtTableTypes'

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
      <div ref={scrollRef} className="kt-table__scroll" style={tableStyles.wrapper}>
        <table style={{ ...tableStyles.table, minWidth: props.tableMinWidth }}>
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

export { KtTable, type Column, type KtTableProps }

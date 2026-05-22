import React, { useState, useCallback } from 'react'

interface Column<T> {
  key: string
  title: string
  width?: number | string
  render?: (record: T, index: number) => React.ReactNode
}

interface KtTableProps<T> {
  columns: Column<T>[]
  data: T[]
  rowKey: string | ((record: T) => string)
  onRowClick?: (record: T) => void
  selectedRowKey?: string
  emptyText?: string
}

const styles = {
  table: {
    width: '100%',
    borderCollapse: 'collapse' as const,
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
  },
  row: {
    height: 48,
    background: 'transparent',
    borderBottom: '1px solid rgba(255,255,255,0.05)',
    cursor: 'pointer',
    transition: 'background 150ms ease',
  },
  cell: {
    padding: '0 16px',
    fontSize: 14,
    color: 'rgba(255,255,255,0.85)',
    overflow: 'hidden' as const,
    textOverflow: 'ellipsis' as const,
    whiteSpace: 'nowrap' as const,
  },
  empty: {
    textAlign: 'center' as const,
    color: 'rgba(255,255,255,0.4)',
    padding: '32px 0',
    fontSize: 14,
  },
} as const

function getRowKey<T>(record: T, rowKey: string | ((record: T) => string)): string {
  return typeof rowKey === 'function'
    ? rowKey(record)
    : String((record as Record<string, unknown>)[rowKey])
}

function KtTable<T>({
  columns,
  data,
  rowKey,
  onRowClick,
  selectedRowKey,
  emptyText = '暂无数据',
}: KtTableProps<T>) {
  const [hoveredKey, setHoveredKey] = useState<string | null>(null)

  const handleMouseEnter = useCallback((key: string) => {
    setHoveredKey(key)
  }, [])

  const handleMouseLeave = useCallback(() => {
    setHoveredKey(null)
  }, [])

  return (
    <table style={styles.table}>
      <thead>
        <tr>
          {columns.map((col) => (
            <th key={col.key} style={{ ...styles.headerCell, width: col.width }}>
              {col.title}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {data.length === 0 ? (
          <tr>
            <td colSpan={columns.length} style={styles.empty}>
              {emptyText}
            </td>
          </tr>
        ) : (
          data.map((record, index) => {
            const key = getRowKey(record, rowKey)
            const isSelected = selectedRowKey === key
            const isHovered = hoveredKey === key

            let rowBg = 'transparent'
            if (isSelected) rowBg = 'rgba(90,176,254,0.05)'
            else if (isHovered) rowBg = 'rgba(255,255,255,0.03)'

            return (
              <tr
                key={key}
                style={{ ...styles.row, background: rowBg }}
                onClick={() => onRowClick?.(record)}
                onMouseEnter={() => handleMouseEnter(key)}
                onMouseLeave={handleMouseLeave}
              >
                {columns.map((col) => (
                  <td key={col.key} style={{ ...styles.cell, width: col.width }}>
                    {col.render
                      ? col.render(record, index)
                      : String((record as Record<string, unknown>)[col.key] ?? '')}
                  </td>
                ))}
              </tr>
            )
          })
        )}
      </tbody>
    </table>
  )
}

export type { Column, KtTableProps }
export { KtTable }

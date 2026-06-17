import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { KtTable } from '../KtTable'
import type { Column } from '../KtTable'

interface DemoRow {
  id: string
  name: string
  note: string
}

const rows: DemoRow[] = [
  { id: '1', name: 'Dale Hutton', note: '第一条测试备注' },
  { id: '2', name: '吉俊鸿', note: '第二条测试备注' },
]

const columns: Column<DemoRow>[] = [
  { key: 'name', title: '名称', width: 120, minWidth: 80, maxWidth: 240 },
  { key: 'note', title: '备注', width: 160 },
]

describe('KtTable', () => {
  it('changes column width by dragging the resize handle', () => {
    render(<KtTable<DemoRow> columns={columns} data={rows} rowKey="id" />)

    const header = screen.getByText('名称').closest('th')
    const handle = screen.getByRole('separator', { name: '调整名称列宽' })

    fireEvent.mouseDown(handle, { clientX: 120 })
    fireEvent.mouseMove(document, { clientX: 180 })
    fireEvent.mouseUp(document)

    expect(header).toHaveStyle({ width: '180px' })
  })

  it('renders expanded row content below the selected record', () => {
    render(
      <KtTable<DemoRow>
        columns={columns}
        data={rows}
        rowKey="id"
        isRowExpanded={(row) => row.id === '2'}
        expandedRowRender={(row) => <div>{`展开详情：${row.note}`}</div>}
      />
    )

    expect(screen.getByText('展开详情：第二条测试备注')).toBeInTheDocument()
  })
})

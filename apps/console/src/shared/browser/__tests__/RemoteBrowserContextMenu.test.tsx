import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import RemoteBrowserContextMenu from '../RemoteBrowserContextMenu'

describe('RemoteBrowserContextMenu', () => {
  it('shows browser actions without save or developer commands', () => {
    render(
      <RemoteBrowserContextMenu
        position={{ x: 10, y: 20 }}
        canCopy
        canEdit
        onAction={vi.fn()}
        onClose={vi.fn()}
      />
    )

    expect(screen.getByRole('menuitem', { name: '复制' })).toBeEnabled()
    expect(screen.getByRole('menuitem', { name: '粘贴' })).toBeEnabled()
    expect(screen.queryByText(/保存|审查|开发者工具/)).not.toBeInTheDocument()
  })

  it('disables sensitive actions and emits navigation commands', () => {
    const onAction = vi.fn()
    render(
      <RemoteBrowserContextMenu
        position={{ x: 10, y: 20 }}
        canCopy={false}
        canEdit={false}
        onAction={onAction}
        onClose={vi.fn()}
      />
    )

    expect(screen.getByRole('menuitem', { name: '复制' })).toBeDisabled()
    expect(screen.getByRole('menuitem', { name: '剪切' })).toBeDisabled()
    fireEvent.click(screen.getByRole('menuitem', { name: '刷新' }))
    expect(onAction).toHaveBeenCalledWith('reload')
  })
})

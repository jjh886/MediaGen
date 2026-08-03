import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'
import type { PageId } from '../../../App'
import AppShell from '../AppShell'

vi.mock('../../../features/tasks/components/GenerationQueueList', () => ({
  default: () => <div>生成队列页面</div>,
}))
vi.mock('../../../features/accounts/components/AccountPage', () => ({
  default: () => <div>账号页面</div>,
}))
vi.mock('../../../features/weixin/components/WeixinPage', () => ({
  default: () => <div>微信页面</div>,
}))
vi.mock('../../../features/auth/components/AuthSessionMenu', () => ({
  AuthSessionMenu: () => null,
}))
vi.mock('../../../features/settings/components/SystemControlCard', () => ({
  SystemControlMenu: () => null,
}))
vi.mock('../StatusBar', () => ({ default: () => null }))

function TestShell() {
  const [page, setPage] = useState<PageId>('accounts')
  return (
    <AppShell
      currentPage={page}
      verificationTaskId={null}
      logTaskId={null}
      onNavigate={setPage}
      onOpenVerification={vi.fn()}
      onOpenTaskLogs={vi.fn()}
    />
  )
}

describe('AppShell 生成队列导航', () => {
  it('显示独立入口并可打开生成队列页面', () => {
    render(<TestShell />)

    const queueButton = screen.getByRole('button', { name: '生成队列' })
    fireEvent.click(queueButton)

    expect(screen.getByText('生成队列页面')).toBeInTheDocument()
    expect(queueButton).toHaveClass('sidebar__item--active')
  })

  it('显示微信入口并可打开微信页面', () => {
    render(<TestShell />)

    const weixinButton = screen.getByRole('button', { name: '微信' })
    fireEvent.click(weixinButton)

    expect(screen.getByText('微信页面')).toBeInTheDocument()
    expect(weixinButton).toHaveClass('sidebar__item--active')
  })
})

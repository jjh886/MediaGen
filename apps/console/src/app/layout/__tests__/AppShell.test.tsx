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

    const queueButton = screen.getByRole('link', { name: '生成队列' })
    fireEvent.click(queueButton)

    expect(screen.getByText('生成队列页面')).toBeInTheDocument()
    expect(queueButton).toHaveClass('sidebar__item--active')
  })

  it('显示微信入口并可打开微信页面', () => {
    render(<TestShell />)

    const weixinButton = screen.getByRole('link', { name: '微信' })
    fireEvent.click(weixinButton)

    expect(screen.getByText('微信页面')).toBeInTheDocument()
    expect(weixinButton).toHaveClass('sidebar__item--active')
  })
  it('桌面模式隐藏后台用户入口，分组可折叠且窄菜单仍可导航', () => {
    render(<TestShell />)
    expect(screen.queryByRole('link', { name: '后台用户' })).not.toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: '主导航' })).toBeInTheDocument()
    const group = screen.getByRole('button', { name: '开发接入' })
    fireEvent.click(group)
    expect(screen.queryByRole('link', { name: '接口文档' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '收起菜单' }))
    expect(screen.getByRole('link', { name: '接口文档' })).toHaveAttribute('href', '/api-docs')
    fireEvent.click(screen.getByRole('link', { name: '微信' }))
    expect(screen.getByText('微信页面')).toBeInTheDocument()
  })
  it('Linux 模式显示用户管理，桌面直接访问该路由会回到账号页', () => {
    const navigate = vi.fn()
    const props = {
      currentPage: 'admin-users' as PageId,
      verificationTaskId: null,
      logTaskId: null,
      onNavigate: navigate,
      onOpenVerification: vi.fn(),
      onOpenTaskLogs: vi.fn(),
    }
    const { rerender } = render(<AppShell {...props} />)
    expect(navigate).toHaveBeenCalledWith('accounts')
    expect(screen.getByText('账号页面')).toBeInTheDocument()
    rerender(<AppShell {...props} currentPage="accounts" adminEnabled />)
    expect(screen.getByRole('link', { name: '后台用户' })).toHaveAttribute('href', '/admin-users')
  })
})

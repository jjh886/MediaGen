import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import AccountTable from '../AccountTable'
import type { Account } from '@/shared/types/account'

const account: Account = {
  id: 'account-one',
  name: '测试账号',
  email: 'user@example.com',
  platform: 'codex',
  avatar_url: '',
  tier: 'pro',
  login_state: 'available',
  created_at: '',
  used_today: 2,
  daily_limit: 5,
  used_total: 9,
  total_limit: 0,
  text_used_today: 3,
  text_daily_limit: 10,
  text_used_total: 20,
  text_total_limit: 100,
  task_interval_seconds: 0,
  concurrency_limit: 1,
  quota_windows: [
    { name: '每周', task_kind: 'text', remaining_percent: 51, reset_at: '2026-09-27T02:23:36Z' },
  ],
}
function callbacks() {
  return {
    onConfigure: vi.fn(),
    onRelogin: vi.fn(),
    onToggle: vi.fn(),
    onDelete: vi.fn(),
    onRefreshQuota: vi.fn(),
    onRefreshAllQuota: vi.fn(),
  }
}

describe('紧凑账号列表', () => {
  it('合并身份与两类用量，保留恢复时间和全部操作', () => {
    const handlers = callbacks()
    render(<AccountTable accounts={[account]} {...handlers} />)
    expect(screen.getAllByRole('columnheader')).toHaveLength(9)
    expect(screen.queryByText('平台剩余额度')).not.toBeInTheDocument()
    expect(screen.getByText('剩余额度')).toBeInTheDocument()
    const cells = within(screen.getAllByRole('row')[1]).getAllByRole('cell')
    expect(cells[0]).toHaveTextContent('测试账号')
    expect(cells[0]).toHaveTextContent('user@example.com')
    expect(cells[0]).toHaveTextContent('pro')
    expect(cells[3]).toHaveTextContent('今日2/5累计9/不限')
    expect(cells[4]).toHaveTextContent('今日3/10累计20/100')
    expect(screen.getByRole('progressbar', { name: '每周剩余额度' })).toHaveAttribute('value', '51')
    expect(
      screen.getByTitle(`恢复：${new Date(account.quota_windows![0].reset_at!).toLocaleString()}`)
    ).toBeInTheDocument()
    for (const [label, callback] of [
      ['配置', handlers.onConfigure],
      ['重新登录', handlers.onRelogin],
      ['停用', handlers.onToggle],
      ['删除', handlers.onDelete],
    ] as const) {
      fireEvent.click(screen.getByRole('button', { name: label, exact: true }))
      expect(callback).toHaveBeenCalledWith(account)
    }
  })

  it('显示代理名称、脱敏地址、未设置和停用状态', () => {
    render(
      <AccountTable
        accounts={[
          { ...account, id: 'saved', proxy_id: 'proxy-1' },
          {
            ...account,
            id: 'custom',
            proxy_url: 'socks5://secret-user:secret-password@127.0.0.1:1080',
          },
          { ...account, id: 'none' },
          { ...account, id: 'missing', proxy_id: 'missing-proxy' },
          { ...account, id: 'invalid', proxy_url: 'http://user:secret-password@' },
        ]}
        proxies={[
          {
            id: 'proxy-1',
            name: '香港节点',
            server: 'http://proxy.example.com:8080',
            username: '',
            password_configured: false,
            enabled: false,
            created_at: '',
          },
        ]}
        {...callbacks()}
      />
    )
    expect(screen.getByText('香港节点')).toBeInTheDocument()
    expect(screen.getByText('（已停用）')).toBeInTheDocument()
    expect(screen.getByTitle('http://proxy.example.com:8080')).toHaveTextContent('香港节点')
    expect(screen.getByText('socks5://127.0.0.1:1080')).toBeInTheDocument()
    expect(screen.getByText('未设置')).toBeInTheDocument()
    expect(screen.getByText('代理 missing-proxy')).toBeInTheDocument()
    expect(screen.getByText('独立代理')).toBeInTheDocument()
    expect(document.body.innerHTML).not.toContain('secret-password')
    expect(document.body.innerHTML).not.toContain('secret-user')
  })

  it('图标可刷新单账号或当前列表，刷新期间禁用并显示忙碌状态', () => {
    const handlers = callbacks()
    const view = render(<AccountTable accounts={[account]} {...handlers} />)
    const single = screen.getByRole('button', { name: '刷新额度 测试账号' })
    expect(single).toHaveTextContent('')
    fireEvent.click(single)
    expect(handlers.onRefreshQuota).toHaveBeenCalledWith(account)
    fireEvent.click(screen.getByRole('button', { name: '刷新当前列表账号额度' }))
    expect(handlers.onRefreshAllQuota).toHaveBeenCalledOnce()
    view.rerender(<AccountTable accounts={[account]} {...handlers} quotaBusy="all" />)
    expect(single).toBeDisabled()
    expect(single).toHaveAttribute('aria-busy', 'true')
    expect(screen.getByRole('button', { name: '刷新当前列表账号额度' })).toBeDisabled()
    fireEvent.click(single)
    expect(handlers.onRefreshQuota).toHaveBeenCalledOnce()
  })
})

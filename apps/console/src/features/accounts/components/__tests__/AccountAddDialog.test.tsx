import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountAddDialog from '../AccountAddDialog'

const mocks = vi.hoisted(() => ({
  startAccountLoginSession: vi.fn(),
  cancelBrowserSession: vi.fn(),
}))

vi.mock('../../services/account-service', () => ({
  startAccountLoginSession: (...args: unknown[]) => mocks.startAccountLoginSession(...args),
}))

vi.mock('@/shared/browser/RemoteBrowserPanel', () => ({
  default: () => <div>远程浏览器画面</div>,
}))

vi.mock('@/shared/services/browser-session-service', () => ({
  browserSessionHistory: vi.fn(),
  cancelBrowserSession: (...args: unknown[]) => mocks.cancelBrowserSession(...args),
  completeBrowserSession: vi.fn(),
}))

describe('AccountAddDialog', () => {
  beforeEach(() => {
    mocks.startAccountLoginSession.mockReset()
    mocks.cancelBrowserSession.mockReset()
  })

  it('添加账号点击打开登录页后创建会话，失败时显示服务端错误', async () => {
    mocks.startAccountLoginSession.mockRejectedValue(
      'HTTP 403 {"error":{"code":"unsupported_country_region_territory"}}'
    )

    render(<AccountAddDialog open platform="codex" onClose={vi.fn()} onSuccess={vi.fn()} />)
    fireEvent.click(screen.getByText('打开登录页'))

    await waitFor(() => {
      expect(screen.getByText(/unsupported_country_region_territory/)).toBeInTheDocument()
    })
    expect(mocks.startAccountLoginSession).toHaveBeenCalledWith('codex', undefined, '')
    expect(screen.queryByText('登录失败，请重试。')).not.toBeInTheDocument()
  })

  it('添加账号时会把启用的 SOCKS5H 代理传给服务端会话', async () => {
    mocks.startAccountLoginSession.mockResolvedValue({
      id: 'session-1',
      platform: 'codex',
      status: 'active',
      viewport: { width: 1365, height: 768 },
    })

    render(<AccountAddDialog open platform="codex" onClose={vi.fn()} onSuccess={vi.fn()} />)
    fireEvent.click(screen.getByLabelText('为这个账号单独设置代理'))
    fireEvent.change(screen.getByPlaceholderText('127.0.0.1 或 proxy.example.com'), {
      target: { value: 'proxy.example.com' },
    })
    fireEvent.change(screen.getByRole('spinbutton'), { target: { value: '1080' } })
    fireEvent.click(screen.getByText('打开登录页'))

    await waitFor(() => {
      expect(mocks.startAccountLoginSession).toHaveBeenCalledWith(
        'codex',
        undefined,
        'socks5h://proxy.example.com:1080'
      )
    })
  })

  it('创建登录页过程中取消时会清理稍后返回的后端会话', async () => {
    let resolveSession!: (value: unknown) => void
    mocks.startAccountLoginSession.mockReturnValue(
      new Promise((resolve) => {
        resolveSession = resolve
      })
    )
    mocks.cancelBrowserSession.mockResolvedValue(undefined)
    const onClose = vi.fn()

    render(<AccountAddDialog open platform="codex" onClose={onClose} onSuccess={vi.fn()} />)
    fireEvent.click(screen.getByText('打开登录页'))
    fireEvent.click(screen.getByText('取消等待'))
    resolveSession({ id: 'session-late', platform: 'codex', status: 'active' })

    await waitFor(() => {
      expect(mocks.cancelBrowserSession).toHaveBeenCalledWith('session-late')
    })
    expect(onClose).toHaveBeenCalled()
  })
})

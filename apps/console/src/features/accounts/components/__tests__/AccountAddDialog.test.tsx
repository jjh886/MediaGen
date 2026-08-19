import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountAddDialog from '../AccountAddDialog'

const mocks = vi.hoisted(() => ({
  startAccountLoginSession: vi.fn(),
  cancelBrowserSession: vi.fn(),
  completeBrowserSession: vi.fn(),
  fetchProxies: vi.fn(),
}))

vi.mock('../../services/account-service', () => ({
  startAccountLoginSession: (...args: unknown[]) => mocks.startAccountLoginSession(...args),
}))

vi.mock('@/shared/services/proxy-service', () => ({
  fetchAllProxies: (...args: unknown[]) => mocks.fetchProxies(...args),
}))

vi.mock('@/shared/browser/RemoteBrowserPanel', () => ({
  default: () => <div>远程浏览器画面</div>,
}))

vi.mock('@/shared/services/browser-session-service', () => ({
  browserSessionHistory: vi.fn(),
  cancelBrowserSession: (...args: unknown[]) => mocks.cancelBrowserSession(...args),
  completeBrowserSession: (...args: unknown[]) => mocks.completeBrowserSession(...args),
}))

describe('AccountAddDialog', () => {
  beforeEach(() => {
    mocks.startAccountLoginSession.mockReset()
    mocks.cancelBrowserSession.mockReset()
    mocks.completeBrowserSession.mockReset()
    mocks.fetchProxies.mockReset()
    mocks.fetchProxies.mockResolvedValue([])
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
    expect(mocks.startAccountLoginSession).toHaveBeenCalledWith(
      'codex',
      undefined,
      '',
      undefined,
      undefined,
      undefined
    )
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
    fireEvent.change(screen.getByLabelText('账号代理'), { target: { value: 'manual' } })
    fireEvent.change(
      screen.getByPlaceholderText('127.0.0.1、proxy.example.com 或完整 socks5h:// 地址'),
      { target: { value: 'proxy.example.com' } }
    )
    fireEvent.change(screen.getByRole('spinbutton'), { target: { value: '1080' } })
    fireEvent.click(screen.getByText('打开登录页'))

    await waitFor(() => {
      expect(mocks.startAccountLoginSession).toHaveBeenCalledWith(
        'codex',
        undefined,
        'socks5h://proxy.example.com:1080',
        undefined,
        undefined,
        undefined
      )
    })
  })

  it('添加账号时可以选择已保存代理并传递 proxy_id', async () => {
    mocks.fetchProxies.mockResolvedValue([
      {
        id: 'proxy-1',
        name: '住宅代理',
        server: 'socks5h://proxy.example:1080',
        username: 'user',
        enabled: true,
        created_at: '2026-06-29T00:00:00Z',
      },
    ])
    mocks.startAccountLoginSession.mockResolvedValue({
      id: 'session-1',
      platform: 'codex',
      status: 'active',
      viewport: { width: 1365, height: 768 },
    })

    render(<AccountAddDialog open platform="codex" onClose={vi.fn()} onSuccess={vi.fn()} />)
    fireEvent.change(await screen.findByLabelText('账号代理'), {
      target: { value: 'saved:proxy-1' },
    })
    fireEvent.click(screen.getByText('打开登录页'))

    await waitFor(() => {
      expect(mocks.startAccountLoginSession).toHaveBeenCalledWith(
        'codex',
        undefined,
        '',
        'proxy-1',
        undefined,
        undefined
      )
    })
  })

  it('添加账号时会把浏览器环境传给服务端会话', async () => {
    mocks.startAccountLoginSession.mockResolvedValue({
      id: 'session-1',
      platform: 'codex',
      status: 'active',
      viewport: { width: 1365, height: 768 },
    })

    render(<AccountAddDialog open platform="codex" onClose={vi.fn()} onSuccess={vi.fn()} />)
    fireEvent.click(screen.getByLabelText('模拟这个账号的系统和浏览器版本'))
    expect(screen.queryByText('Chrome 主版本')).not.toBeInTheDocument()
    expect(screen.getByText(/当前的稳定主版本 150/)).toBeInTheDocument()
    fireEvent.click(screen.getByText('打开登录页'))

    await waitFor(() => {
      expect(mocks.startAccountLoginSession).toHaveBeenCalledWith(
        'codex',
        undefined,
        '',
        undefined,
        { browser_os: 'windows', browser_version: '150' },
        undefined
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

  it('登录检测未通过时保留窗口并显示继续登录提示', async () => {
    mocks.startAccountLoginSession.mockResolvedValue({
      id: 'session-login',
      platform: 'chatgpt',
      status: 'active',
      viewport: { width: 1365, height: 768 },
    })
    mocks.completeBrowserSession.mockRejectedValue(
      new Error('还没有检测到登录成功，请继续在页面里完成登录。')
    )

    render(<AccountAddDialog open platform="chatgpt" onClose={vi.fn()} onSuccess={vi.fn()} />)
    fireEvent.click(screen.getByText('打开登录页'))

    expect(await screen.findByText('远程浏览器画面')).toBeInTheDocument()
    expect(
      await screen.findByText('还没有检测到登录成功，请继续在页面里完成登录。', undefined, {
        timeout: 4500,
      })
    ).toBeInTheDocument()
  })
})

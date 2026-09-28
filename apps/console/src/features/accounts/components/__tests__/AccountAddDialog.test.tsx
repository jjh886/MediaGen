import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountAddDialog from '../AccountAddDialog'

const mocks = vi.hoisted(() => ({
  startAccountLoginSession: vi.fn(),
  cancelBrowserSession: vi.fn(),
  completeBrowserSession: vi.fn(),
  fetchProxies: vi.fn(),
  fetchAccounts: vi.fn(),
  fetchAccount: vi.fn(),
  fetchFingerprintOptions: vi.fn(),
}))

vi.mock('../../services/account-service', () => ({
  startAccountLoginSession: (...args: unknown[]) => mocks.startAccountLoginSession(...args),
}))

vi.mock('@/shared/services/proxy-service', () => ({
  fetchAllProxies: (...args: unknown[]) => mocks.fetchProxies(...args),
  proxyOptionLabel: (proxy: { name: string; bound_account_count?: number }) =>
    `${proxy.name} · 已绑定 ${proxy.bound_account_count ?? 0} 个账号`,
}))

vi.mock('@/shared/services/account-service', () => ({
  fetchAllAccounts: (...args: unknown[]) => mocks.fetchAccounts(...args),
  fetchAccount: (...args: unknown[]) => mocks.fetchAccount(...args),
}))

vi.mock('@/shared/browser/RemoteBrowserPanel', () => ({
  default: () => <div>远程浏览器画面</div>,
}))

vi.mock('@/shared/services/browser-session-service', () => ({
  browserSessionHistory: vi.fn(),
  cancelBrowserSession: (...args: unknown[]) => mocks.cancelBrowserSession(...args),
  completeBrowserSession: (...args: unknown[]) => mocks.completeBrowserSession(...args),
  fetchBrowserFingerprintOptions: () => mocks.fetchFingerprintOptions(),
}))

describe('AccountAddDialog', () => {
  beforeEach(() => {
    mocks.startAccountLoginSession.mockReset()
    mocks.cancelBrowserSession.mockReset()
    mocks.completeBrowserSession.mockReset()
    mocks.fetchProxies.mockReset()
    mocks.fetchProxies.mockResolvedValue([])
    mocks.fetchAccounts.mockResolvedValue([])
    mocks.fetchFingerprintOptions.mockResolvedValue({
      browser_version: '154',
      installed_browser_version: '146',
      os: 'linux',
    })
  })

  it('读取浏览器版本失败时显示缺少运行时的具体修复提示', async () => {
    const message = '未检测到 CloakBrowser；请执行 node scripts/ensure-cloakbrowser-runtime.mjs'
    mocks.fetchFingerprintOptions.mockRejectedValueOnce(new Error(message))
    render(<AccountAddDialog open platform="codex" onClose={vi.fn()} onSuccess={vi.fn()} />)
    expect(await screen.findByRole('alert')).toHaveTextContent(message)
    expect(screen.queryByText('Chrome 154 · 最新稳定版')).not.toBeInTheDocument()
  })

  it('读取浏览器版本发生未知错误时保留通用提示', async () => {
    mocks.fetchFingerprintOptions.mockRejectedValueOnce(null)
    render(<AccountAddDialog open platform="codex" onClose={vi.fn()} onSuccess={vi.fn()} />)
    expect(await screen.findByRole('alert')).toHaveTextContent('无法读取浏览器版本，请稍后重试。')
  })

  it('显示插件返回的设备授权验证码提示', async () => {
    mocks.startAccountLoginSession.mockResolvedValue({
      id: 'device-session',
      platform: 'grok',
      status: 'active',
      metadata: { login_instructions: '请在授权页面输入 ABCD-1234' },
    })
    render(<AccountAddDialog open platform="grok" onClose={vi.fn()} onSuccess={vi.fn()} />)
    await screen.findByText('Chrome 154 · 最新稳定版')
    fireEvent.click(screen.getByText('打开登录页'))
    expect(await screen.findByText('请在授权页面输入 ABCD-1234')).toBeInTheDocument()
  })

  it('添加账号点击打开登录页后创建会话，失败时显示服务端错误', async () => {
    mocks.startAccountLoginSession.mockRejectedValue(
      'HTTP 403 {"error":{"code":"unsupported_country_region_territory"}}'
    )

    render(<AccountAddDialog open platform="codex" onClose={vi.fn()} onSuccess={vi.fn()} />)
    await screen.findByText('Chrome 154 · 最新稳定版')
    fireEvent.click(screen.getByText('打开登录页'))

    await waitFor(() => {
      expect(screen.getByText(/unsupported_country_region_territory/)).toBeInTheDocument()
    })
    expect(mocks.startAccountLoginSession).toHaveBeenCalledWith(
      'codex',
      undefined,
      '',
      undefined,
      { browser_os: 'linux', browser_version: '154' },
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
    await screen.findByText('Chrome 154 · 最新稳定版')
    fireEvent.change(screen.getByLabelText('账号代理'), { target: { value: 'manual' } })
    fireEvent.change(
      screen.getByPlaceholderText('主机、完整代理 URL 或 username:password@ip:port'),
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
        { browser_os: 'linux', browser_version: '154' },
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
    await screen.findByText('Chrome 154 · 最新稳定版')
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
        { browser_os: 'linux', browser_version: '154' },
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
    await screen.findByText('Chrome 154 · 最新稳定版')
    expect(screen.getByRole('button', { name: 'Linux' })).toHaveAttribute('aria-pressed', 'true')
    for (const os of ['Windows', 'macOS', 'Linux']) {
      expect(
        screen.getByRole('button', { name: os }).querySelector('[data-system-icon]')
      ).not.toBeNull()
    }
    expect(screen.queryByRole('button', { name: '服务器默认' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Windows' }))
    fireEvent.click(screen.getByRole('button', { name: 'Chrome 153' }))
    fireEvent.click(screen.getByText('打开登录页'))

    await waitFor(() => {
      expect(mocks.startAccountLoginSession).toHaveBeenCalledWith(
        'codex',
        undefined,
        '',
        undefined,
        { browser_os: 'windows', browser_version: '153' },
        undefined,
        undefined
      )
    })
  })

  it('选择已有账号后持续复用其资料并锁定代理和指纹', async () => {
    mocks.fetchAccounts.mockResolvedValue([
      {
        id: 'chatgpt-1',
        platform: 'chatgpt',
        name: '我的 ChatGPT',
        profile_dir: '/profiles/chatgpt-1',
        browser_os: 'macos',
        browser_version: '145',
      },
    ])
    mocks.startAccountLoginSession.mockResolvedValue({ id: 'shared-session', status: 'active' })
    render(<AccountAddDialog open platform="codex" onClose={vi.fn()} onSuccess={vi.fn()} />)
    fireEvent.change(await screen.findByLabelText('复用已有账号浏览器资料'), {
      target: { value: 'chatgpt-1' },
    })
    expect(screen.queryByLabelText('账号代理')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'macOS' })).toBeDisabled()
    fireEvent.click(screen.getByText('打开登录页'))
    await waitFor(() =>
      expect(mocks.startAccountLoginSession).toHaveBeenCalledWith(
        'codex',
        undefined,
        '',
        undefined,
        undefined,
        undefined,
        'chatgpt-1'
      )
    )
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
    await screen.findByText('Chrome 154 · 最新稳定版')
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
    await screen.findByText('Chrome 154 · 最新稳定版')
    fireEvent.click(screen.getByText('打开登录页'))

    expect(await screen.findByText('远程浏览器画面')).toBeInTheDocument()
    expect(
      await screen.findByText('还没有检测到登录成功，请继续在页面里完成登录。', undefined, {
        timeout: 4500,
      })
    ).toBeInTheDocument()
  })
})

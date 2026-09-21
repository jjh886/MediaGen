import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AuthGate from '../AuthGate'
import { getAuthStatus, loginAdmin } from '../../../../shared/services/auth-service'

vi.mock('../../../../shared/services/auth-service', () => ({
  getAuthStatus: vi.fn(),
  loginAdmin: vi.fn(),
}))

describe('AuthGate', () => {
  beforeEach(() => {
    vi.mocked(getAuthStatus).mockReset()
    vi.mocked(loginAdmin).mockReset()
  })

  it('已登录时直接显示控制台内容', async () => {
    vi.mocked(getAuthStatus).mockResolvedValue({
      supported: true,
      configured: true,
      authenticated: true,
      username: 'admin',
    })

    render(
      <AuthGate>
        <div>控制台内容</div>
      </AuthGate>
    )

    await expect(screen.findByText('控制台内容')).resolves.toBeInTheDocument()
  })

  it('未登录时先显示登录页，登录成功后进入控制台', async () => {
    vi.mocked(getAuthStatus).mockResolvedValue({
      supported: true,
      configured: true,
      authenticated: false,
    })
    vi.mocked(loginAdmin).mockResolvedValue({
      supported: true,
      configured: true,
      authenticated: true,
      username: 'admin',
    })

    render(
      <AuthGate>
        <div>账号管理</div>
      </AuthGate>
    )

    await screen.findByRole('heading', { name: '登录 MediaGen' })
    fireEvent.change(screen.getByPlaceholderText('请输入管理员密码'), {
      target: { value: 'secret' },
    })
    fireEvent.click(screen.getByRole('button', { name: '登录管理台' }))

    await waitFor(() =>
      expect(loginAdmin).toHaveBeenCalledWith({ username: 'admin', password: 'secret' })
    )
    await expect(screen.findByText('账号管理')).resolves.toBeInTheDocument()
  })

  it('macOS 和 Windows 不进入管理员登录页', async () => {
    vi.mocked(getAuthStatus).mockResolvedValue({
      supported: false,
      configured: false,
      authenticated: false,
    })

    render(
      <AuthGate>
        <div>本机控制台</div>
      </AuthGate>
    )

    await expect(screen.findByText('本机控制台')).resolves.toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: '登录 MediaGen' })).not.toBeInTheDocument()
  })
  it('Linux 未初始化管理员时不能直接进入控制台', async () => {
    vi.mocked(getAuthStatus).mockResolvedValue({
      supported: true,
      configured: false,
      authenticated: false,
    })
    render(
      <AuthGate>
        <div>管理台敏感内容</div>
      </AuthGate>
    )
    expect(await screen.findByRole('heading', { name: '初始化管理员' })).toBeInTheDocument()
    expect(screen.queryByText('管理台敏感内容')).not.toBeInTheDocument()
  })
  it('连接失败先显示重试，不能把桌面服务故障误报为需要密码', async () => {
    vi.mocked(getAuthStatus)
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce({ supported: false, configured: true, authenticated: false })
    render(<AuthGate>{(status) => <div>本机状态 {String(status.supported)}</div>}</AuthGate>)
    expect(await screen.findByRole('heading', { name: '无法连接服务' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: '登录 MediaGen' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '重试连接' }))
    expect(await screen.findByText('本机状态 false')).toBeInTheDocument()
    fireEvent(window, new Event('mediagen:auth-expired'))
    expect(screen.getByText('本机状态 false')).toBeInTheDocument()
  })
  it('Linux 会话失效回到登录页，密码可临时显示且登录错误可见', async () => {
    vi.mocked(getAuthStatus).mockResolvedValue({
      supported: true,
      configured: true,
      authenticated: true,
    })
    vi.mocked(loginAdmin).mockRejectedValue(new Error('账号或密码不正确'))
    render(
      <AuthGate>
        <div>控制台</div>
      </AuthGate>
    )
    await screen.findByText('控制台')
    fireEvent(window, new Event('mediagen:auth-expired'))
    const password = await screen.findByPlaceholderText('请输入管理员密码')
    fireEvent.click(screen.getByRole('button', { name: '显示密码' }))
    expect(password).toHaveAttribute('type', 'text')
    fireEvent.change(password, { target: { value: 'incorrect' } })
    fireEvent.click(screen.getByRole('button', { name: '登录管理台' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('账号或密码不正确')
  })
})

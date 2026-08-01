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
})

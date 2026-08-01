import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getAuthStatus, logoutAdmin } from '../../../../shared/services/auth-service'
import { AuthSessionMenu } from '../AuthSessionMenu'

vi.mock('../../../../shared/services/auth-service', () => ({
  getAuthStatus: vi.fn(),
  logoutAdmin: vi.fn(),
}))

describe('AuthSessionMenu', () => {
  beforeEach(() => {
    vi.mocked(getAuthStatus).mockReset()
    vi.mocked(logoutAdmin).mockReset()
  })

  it('macOS 和 Windows 即使存在旧登录配置也隐藏退出按钮', async () => {
    vi.mocked(getAuthStatus).mockResolvedValue({
      supported: false,
      configured: true,
      authenticated: true,
    })

    render(<AuthSessionMenu />)

    await waitFor(() => expect(getAuthStatus).toHaveBeenCalledOnce())
    expect(screen.queryByRole('button', { name: '退出登录' })).not.toBeInTheDocument()
  })

  it('Linux 服务端配置管理员登录时显示退出按钮', async () => {
    vi.mocked(getAuthStatus).mockResolvedValue({
      supported: true,
      configured: true,
      authenticated: true,
      username: 'admin',
    })

    render(<AuthSessionMenu />)

    await expect(screen.findByRole('button', { name: '退出登录' })).resolves.toBeInTheDocument()
  })
})

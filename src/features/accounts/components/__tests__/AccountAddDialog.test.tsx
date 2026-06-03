import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountAddDialog from '../AccountAddDialog'

const mockAddAccountByLogin = vi.fn()
const mockReloginAccount = vi.fn()

vi.mock('../../services/account-service', () => ({
  addAccountByLogin: (...args: unknown[]) => mockAddAccountByLogin(...args),
  reloginAccount: (...args: unknown[]) => mockReloginAccount(...args),
}))

describe('AccountAddDialog', () => {
  beforeEach(() => {
    mockAddAccountByLogin.mockReset()
    mockReloginAccount.mockReset()
  })

  it('添加账号失败时显示 Tauri 返回的字符串错误', async () => {
    mockAddAccountByLogin.mockRejectedValue(
      'HTTP 403 {"error":{"code":"unsupported_country_region_territory"}}'
    )

    render(<AccountAddDialog open platform="codex" onClose={vi.fn()} onSuccess={vi.fn()} />)

    fireEvent.click(screen.getByRole('button', { name: '开始登录' }))

    await waitFor(() => {
      expect(screen.getByText(/unsupported_country_region_territory/)).toBeInTheDocument()
    })
    expect(screen.queryByText('登录失败，请重试。')).not.toBeInTheDocument()
  })
})

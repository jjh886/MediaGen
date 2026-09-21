import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AdminUsersPage from '../AdminUsersPage'
const mocks = vi.hoisted(() => ({
  fetch: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  reset: vi.fn(),
  remove: vi.fn(),
}))
vi.mock('@/shared/services/admin-users-service', () => ({
  fetchAdminUsers: mocks.fetch,
  createAdminUser: mocks.create,
  updateAdminUser: mocks.update,
  resetAdminPassword: mocks.reset,
  deleteAdminUser: mocks.remove,
}))
vi.mock('@/shared/services/auth-service', () => ({
  getAuthStatus: vi.fn().mockResolvedValue({
    supported: true,
    configured: true,
    authenticated: true,
    user_id: 'admin',
  }),
}))
const users = [
  {
    id: 'admin',
    username: 'admin',
    display_name: '管理员',
    enabled: true,
    created_at: '2026-09-21T00:00:00Z',
    last_login_at: null,
  },
  {
    id: 'other',
    username: 'operator',
    display_name: '操作员',
    enabled: true,
    created_at: '2026-09-21T00:00:00Z',
    last_login_at: null,
  },
]
beforeEach(() => {
  vi.clearAllMocks()
  mocks.fetch.mockResolvedValue(users)
  mocks.create.mockResolvedValue({ id: 'new' })
  mocks.update.mockResolvedValue(users[1])
  mocks.reset.mockResolvedValue({ ok: true })
  mocks.remove.mockResolvedValue(undefined)
})
describe('后台用户管理', () => {
  it('新增前校验密码，编辑名称并能重置其他管理员密码', async () => {
    render(<AdminUsersPage />)
    await screen.findByText('operator')
    expect(screen.getByRole('button', { name: '删除 admin' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: '新增用户' }))
    fireEvent.change(screen.getByLabelText('用户名'), { target: { value: 'new-user' } })
    fireEvent.change(screen.getByLabelText('新密码'), { target: { value: 'short' } })
    fireEvent.change(screen.getByLabelText('确认密码'), { target: { value: 'short' } })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    expect(await screen.findByText('密码至少 12 个字符')).toBeInTheDocument()
    expect(mocks.create).not.toHaveBeenCalled()
    for (const label of ['新密码', '确认密码'])
      fireEvent.change(screen.getByLabelText(label), { target: { value: 'a-long-password' } })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    await waitFor(() =>
      expect(mocks.create).toHaveBeenCalledWith({
        username: 'new-user',
        display_name: '',
        password: 'a-long-password',
      })
    )
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: '编辑 operator' }))
    fireEvent.change(screen.getByLabelText('显示名称'), { target: { value: '新名字' } })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith(
        'other',
        expect.objectContaining({ display_name: '新名字' })
      )
    )
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: '重置密码 operator' }))
    for (const label of ['新密码', '确认密码'])
      fireEvent.change(screen.getByLabelText(label), { target: { value: 'another-password' } })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    await waitFor(() => expect(mocks.reset).toHaveBeenCalledWith('other', 'another-password'))
  })
  it('删除需要确认，服务端拒绝时展示错误', async () => {
    mocks.remove.mockRejectedValueOnce(new Error('必须保留至少一个启用的管理员'))
    render(<AdminUsersPage />)
    await screen.findByText('operator')
    fireEvent.click(screen.getByRole('button', { name: '删除 operator' }))
    expect(mocks.remove).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: '删除', exact: true }))
    expect(await screen.findByRole('alert')).toHaveTextContent('必须保留至少一个启用的管理员')
  })
})

import { beforeEach, expect, it, vi } from 'vitest'
import {
  fetchAdminUsers,
  createAdminUser,
  updateAdminUser,
  resetAdminPassword,
  deleteAdminUser,
} from '../admin-users-service'
vi.mock('../http-api-client', () => ({
  apiGet: vi.fn(),
  apiPost: vi.fn(),
  apiPatch: vi.fn(),
  apiDelete: vi.fn(),
}))
import { apiGet, apiPost, apiPatch, apiDelete } from '../http-api-client'
beforeEach(() => vi.clearAllMocks())
it('用户增删改和密码重置使用独立管理接口并编码资源 ID', async () => {
  await fetchAdminUsers()
  expect(apiGet).toHaveBeenCalledWith('/api/admin-users')
  await createAdminUser({ username: 'operator', display_name: '操作员', password: 'long-password' })
  expect(apiPost).toHaveBeenCalledWith('/api/admin-users', {
    username: 'operator',
    display_name: '操作员',
    password: 'long-password',
  })
  await updateAdminUser('a/b', { enabled: false })
  expect(apiPatch).toHaveBeenCalledWith('/api/admin-users/a%2Fb', { enabled: false })
  await resetAdminPassword('a/b', 'changed-password')
  expect(apiPost).toHaveBeenCalledWith('/api/admin-users/a%2Fb/password', {
    password: 'changed-password',
  })
  await deleteAdminUser('a/b')
  expect(apiDelete).toHaveBeenCalledWith('/api/admin-users/a%2Fb')
})

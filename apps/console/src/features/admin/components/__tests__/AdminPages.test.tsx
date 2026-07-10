import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiKeysPage, ProxiesPage } from '../AdminPages'

const mocks = vi.hoisted(() => ({
  fetchApiKeys: vi.fn(),
  createApiKey: vi.fn(),
  deleteApiKey: vi.fn(),
  fetchProxies: vi.fn(),
  createProxy: vi.fn(),
  deleteProxy: vi.fn(),
  testProxy: vi.fn(),
}))

vi.mock('../../services/admin-service', () => ({
  fetchApiKeys: (...args: unknown[]) => mocks.fetchApiKeys(...args),
  createApiKey: (...args: unknown[]) => mocks.createApiKey(...args),
  deleteApiKey: (...args: unknown[]) => mocks.deleteApiKey(...args),
  fetchProxies: (...args: unknown[]) => mocks.fetchProxies(...args),
  createProxy: (...args: unknown[]) => mocks.createProxy(...args),
  deleteProxy: (...args: unknown[]) => mocks.deleteProxy(...args),
  testProxy: (...args: unknown[]) => mocks.testProxy(...args),
}))

describe('AdminPages', () => {
  beforeEach(() => {
    mocks.fetchApiKeys.mockReset()
    mocks.createApiKey.mockReset()
    mocks.deleteApiKey.mockReset()
    mocks.fetchProxies.mockReset()
    mocks.createProxy.mockReset()
    mocks.deleteProxy.mockReset()
    mocks.testProxy.mockReset()
    mocks.fetchApiKeys.mockResolvedValue([
      {
        id: 'key-1',
        name: 'Web 用户',
        masked_key: 'sk-****1234',
        created_at: '2026-06-29T00:00:00Z',
      },
    ])
    mocks.createApiKey.mockResolvedValue({
      id: 'key-2',
      key: 'mg_generated_key_1234',
      masked_key: 'mg_g****1234',
    })
    mocks.fetchProxies.mockResolvedValue([
      {
        id: 'proxy-1',
        name: '默认代理',
        server: 'socks5h://proxy.example:1080',
        username: '',
        password_configured: false,
        enabled: true,
        created_at: '2026-06-29T00:00:00Z',
      },
    ])
    mocks.createProxy.mockResolvedValue(undefined)
    mocks.testProxy.mockResolvedValue({
      ok: true,
      status: 204,
      url: 'https://www.gstatic.com/generate_204',
    })
  })

  it('API Key 页面使用弹窗自动生成 Key，并在前端拦截空表单', async () => {
    render(<ApiKeysPage />)

    expect(await screen.findByText('Web 用户')).toBeInTheDocument()
    expect(screen.queryByPlaceholderText('新的 API Key')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '新增 API Key' }))
    expect(screen.queryByLabelText('API Key')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '新增' }))

    expect(await screen.findByText('请填写名称')).toBeInTheDocument()
    expect(mocks.createApiKey).not.toHaveBeenCalled()

    fireEvent.change(screen.getByLabelText('名称'), { target: { value: '新用户' } })
    fireEvent.click(screen.getByRole('button', { name: '新增' }))

    await waitFor(() => {
      expect(mocks.createApiKey).toHaveBeenCalledWith('新用户')
    })
    expect(await screen.findByDisplayValue('mg_generated_key_1234')).toBeInTheDocument()
    expect(mocks.fetchApiKeys).toHaveBeenCalledTimes(2)
  })

  it('代理页面使用弹窗新增，并校验端口和认证字段', async () => {
    render(<ProxiesPage />)

    expect(await screen.findByText('默认代理')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '测试代理 默认代理' }))

    await waitFor(() => {
      expect(mocks.testProxy).toHaveBeenCalledWith('proxy-1')
    })
    expect(await screen.findByText('测试通过（HTTP 204）')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '新增代理' }))
    fireEvent.change(screen.getByLabelText('代理名称'), { target: { value: '新代理' } })
    fireEvent.change(screen.getByLabelText('代理地址'), { target: { value: 'proxy.example' } })
    fireEvent.change(screen.getByLabelText('端口'), { target: { value: '70000' } })
    fireEvent.change(screen.getByLabelText('密码'), { target: { value: 'pass' } })
    fireEvent.click(screen.getByRole('button', { name: '新增' }))

    expect(await screen.findByText('端口必须是 1 到 65535 之间的整数')).toBeInTheDocument()
    expect(screen.getByText('填写密码时也要填写账号')).toBeInTheDocument()
    expect(mocks.createProxy).not.toHaveBeenCalled()

    fireEvent.change(screen.getByLabelText('端口'), { target: { value: '1080' } })
    fireEvent.change(screen.getByLabelText('账号'), { target: { value: 'user' } })
    fireEvent.click(screen.getByRole('button', { name: '新增' }))

    await waitFor(() => {
      expect(mocks.createProxy).toHaveBeenCalledWith({
        name: '新代理',
        server: 'socks5h://proxy.example:1080',
        username: 'user',
        password: 'pass',
      })
    })
    expect(mocks.fetchProxies).toHaveBeenCalledTimes(2)
  })
})

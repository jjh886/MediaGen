import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import AccountConfigModal from '../AccountConfigModal'
import type { Account } from '@/shared/types/account'

vi.mock('@/shared/services/proxy-service', () => ({
  fetchAllProxies: vi.fn().mockResolvedValue([]),
  proxyOptionLabel: vi.fn(),
}))
const account: Account = {
  id: 'one',
  platform: 'codex',
  name: '测试账号',
  email: '',
  avatar_url: '',
  tier: 'free',
  login_state: 'available',
  used_today: 2,
  used_total: 3,
  daily_limit: 5,
  total_limit: 10,
  concurrency_limit: 1,
  task_interval_seconds: 0,
  created_at: '',
  text_concurrency_limit: 2,
  quota_windows: [
    { name: '五小时', task_kind: 'text', remaining_percent: 25, reset_at: '2026-09-22T00:00:00Z' },
  ],
}

describe('账号配置', () => {
  it('独立保存文本和图片额度，后台轮询不覆盖未保存编辑', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined)
    const onClose = vi.fn()
    const view = render(
      <AccountConfigModal open account={account} onSave={onSave} onClose={onClose} />
    )
    expect(screen.queryByText('获取可用模型')).not.toBeInTheDocument()
    expect(screen.getByText(/25%/)).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('文本并发'), { target: { value: '4' } })
    fireEvent.change(screen.getByLabelText('图片 / 媒体每日限额'), { target: { value: '15' } })
    view.rerender(
      <AccountConfigModal
        open
        account={{ ...account, used_today: 3 }}
        onSave={onSave}
        onClose={onClose}
      />
    )
    expect(screen.getByLabelText('文本并发')).toHaveValue(4)
    expect(screen.getByLabelText('图片 / 媒体每日限额')).toHaveValue(15)
    fireEvent.click(screen.getByRole('tab', { name: '代理与浏览器' }))
    expect(screen.getByText(/代理失效会报错/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('tab', { name: '账号与授权' }))
    fireEvent.change(screen.getByLabelText('授权 API Key'), {
      target: { value: 'key-one\nkey-two' },
    })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    await waitFor(() =>
      expect(onSave).toHaveBeenCalledWith(
        'one',
        expect.objectContaining({
          text_concurrency_limit: 4,
          daily_limit: 15,
          concurrency_limit: 1,
          text_daily_limit: 0,
          total_limit: 10,
          api_key: 'key-one\nkey-two',
        })
      )
    )
    expect(onClose).toHaveBeenCalledOnce()
  })

  it('拦截无效限额并显示保存失败，保留未修改的密钥', async () => {
    const onSave = vi.fn().mockRejectedValue(new Error('保存失败'))
    render(<AccountConfigModal open account={account} onSave={onSave} onClose={vi.fn()} />)
    fireEvent.change(screen.getByLabelText('文本并发'), { target: { value: '-1' } })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('文本并发数必须')
    expect(onSave).not.toHaveBeenCalled()
    fireEvent.change(screen.getByLabelText('文本并发'), { target: { value: '0' } })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('保存失败')
    expect(onSave.mock.calls[0][1]).not.toHaveProperty('api_key')
  })
})

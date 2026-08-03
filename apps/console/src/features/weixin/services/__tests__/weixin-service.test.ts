import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  fetchWeixinSettings,
  runWeixinAction,
  saveWeixinSettings,
  startWeixinBinding,
} from '../weixin-service'
import { weixinSettingsUpdateFixture } from '../../test-fixtures'

const mockApiGet = vi.fn()
const mockApiPost = vi.fn()

vi.mock('@/shared/services/http-api-client', () => ({
  apiGet: (...args: unknown[]) => mockApiGet(...args),
  apiPost: (...args: unknown[]) => mockApiPost(...args),
}))

beforeEach(() => {
  mockApiGet.mockReset()
  mockApiPost.mockReset()
})

describe('weixin-service', () => {
  it('读取微信设置使用管理端接口', async () => {
    const response = { status: 'active', models: [] }
    mockApiGet.mockResolvedValue(response)

    await expect(fetchWeixinSettings()).resolves.toBe(response)

    expect(mockApiGet).toHaveBeenCalledWith('/api/weixin/settings')
  })

  it('开始绑定会请求扫码接口', async () => {
    const response = { status: 'wait', sessionId: 'session-1' }
    mockApiPost.mockResolvedValue(response)

    await expect(startWeixinBinding()).resolves.toBe(response)

    expect(mockApiPost).toHaveBeenCalledWith('/api/weixin/binding', {})
  })

  it('转发微信动作请求', async () => {
    const request = { action: 'send_test' }
    mockApiPost.mockResolvedValue({ status: 'queued' })

    await runWeixinAction(request)

    expect(mockApiPost).toHaveBeenCalledWith('/api/weixin/actions', request)
  })

  it('保存设置使用统一 action 格式', async () => {
    mockApiPost.mockResolvedValue({ status: 'saved' })

    await saveWeixinSettings(weixinSettingsUpdateFixture)

    expect(mockApiPost).toHaveBeenCalledWith('/api/weixin/actions', {
      action: 'save_settings',
      settings: weixinSettingsUpdateFixture,
    })
  })
})

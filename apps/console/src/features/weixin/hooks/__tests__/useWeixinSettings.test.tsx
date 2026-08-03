import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useWeixinSettings } from '../useWeixinSettings'
import { weixinSettingsFixture } from '../../test-fixtures'

const mockFetchWeixinSettings = vi.fn()
const mockRunWeixinAction = vi.fn()
const mockSaveWeixinSettings = vi.fn()
const mockStartWeixinBinding = vi.fn()

vi.mock('../../services/weixin-service', () => ({
  fetchWeixinSettings: (...args: unknown[]) => mockFetchWeixinSettings(...args),
  runWeixinAction: (...args: unknown[]) => mockRunWeixinAction(...args),
  saveWeixinSettings: (...args: unknown[]) => mockSaveWeixinSettings(...args),
  startWeixinBinding: (...args: unknown[]) => mockStartWeixinBinding(...args),
}))

beforeEach(() => {
  mockFetchWeixinSettings.mockReset()
  mockRunWeixinAction.mockReset()
  mockSaveWeixinSettings.mockReset()
  mockStartWeixinBinding.mockReset()
})

describe('useWeixinSettings', () => {
  it('加载设置后同步页面草稿', async () => {
    mockFetchWeixinSettings.mockResolvedValue(weixinSettingsFixture)
    const { result } = renderHook(() => useWeixinSettings())

    await act(async () => result.current.load())

    expect(result.current.settings).toEqual(weixinSettingsFixture)
    expect(result.current.draft).toMatchObject({
      generationEnabled: true,
      defaultModelId: 'gpt-image-1',
      defaultMaxImages: 1,
    })
    expect(result.current.loading).toBe(false)
  })

  it('加载失败时展示接口错误', async () => {
    mockFetchWeixinSettings.mockRejectedValue(new Error('微信不可用'))
    const { result } = renderHook(() => useWeixinSettings())

    await act(async () => result.current.load())

    expect(result.current.error).toBe('微信不可用')
    expect(result.current.loading).toBe(false)
  })

  it('草稿为空时更新和保存都不会请求后端', async () => {
    const { result } = renderHook(() => useWeixinSettings())

    act(() => result.current.updateDraft({ defaultMaxImages: 2 }))
    await act(async () => result.current.save())

    expect(result.current.draft).toBeNull()
    expect(mockSaveWeixinSettings).not.toHaveBeenCalled()
  })

  it('更新草稿后保存并重新加载设置', async () => {
    mockFetchWeixinSettings.mockResolvedValue(weixinSettingsFixture)
    mockSaveWeixinSettings.mockResolvedValue({ status: 'saved', message: '已保存' })
    const { result } = renderHook(() => useWeixinSettings())

    await act(async () => result.current.load())
    act(() => result.current.updateDraft({ defaultMaxImages: 3 }))
    await act(async () => result.current.save())

    expect(mockSaveWeixinSettings).toHaveBeenCalledWith(
      expect.objectContaining({ defaultMaxImages: 3 })
    )
    expect(result.current.message).toBe('已保存')
    expect(result.current.saving).toBe(false)
  })

  it('保存失败时保留可读错误', async () => {
    mockFetchWeixinSettings.mockResolvedValue(weixinSettingsFixture)
    mockSaveWeixinSettings.mockRejectedValue('network')
    const { result } = renderHook(() => useWeixinSettings())

    await act(async () => result.current.load())
    await act(async () => result.current.save())

    expect(result.current.error).toBe('微信操作失败。')
    expect(result.current.saving).toBe(false)
  })

  it('执行普通动作成功后刷新设置', async () => {
    mockFetchWeixinSettings.mockResolvedValue(weixinSettingsFixture)
    mockRunWeixinAction.mockResolvedValue({ status: 'queued', message: '测试消息已加入发送队列' })
    const { result } = renderHook(() => useWeixinSettings())

    const actionResult = await act(async () => result.current.action({ action: 'send_test' }))

    expect(actionResult?.status).toBe('queued')
    expect(mockFetchWeixinSettings).toHaveBeenCalled()
    expect(result.current.message).toBe('测试消息已加入发送队列')
  })

  it('执行普通动作失败时返回 null', async () => {
    mockRunWeixinAction.mockRejectedValue(new Error('动作失败'))
    const { result } = renderHook(() => useWeixinSettings())

    const actionResult = await act(async () => result.current.action({ action: 'send_test' }))

    expect(actionResult).toBeNull()
    expect(result.current.error).toBe('动作失败')
    expect(result.current.acting).toBe(false)
  })

  it('开始绑定成功后返回扫码状态', async () => {
    mockFetchWeixinSettings.mockResolvedValue(weixinSettingsFixture)
    mockStartWeixinBinding.mockResolvedValue({
      status: 'wait',
      message: '请扫码',
      sessionId: 'session-1',
    })
    const { result } = renderHook(() => useWeixinSettings())

    const bindResult = await act(async () => result.current.beginBinding())

    expect(bindResult?.sessionId).toBe('session-1')
    expect(result.current.message).toBe('请扫码')
  })

  it('开始绑定失败时返回 null', async () => {
    mockStartWeixinBinding.mockRejectedValue(new Error('二维码失败'))
    const { result } = renderHook(() => useWeixinSettings())

    const bindResult = await act(async () => result.current.beginBinding())

    expect(bindResult).toBeNull()
    expect(result.current.error).toBe('二维码失败')
  })
})

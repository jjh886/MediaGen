import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { BrowserSession } from '@/shared/services/browser-session-service'
import RemoteBrowserPanel from '../RemoteBrowserPanel'

const mocks = vi.hoisted(() => ({ resize: vi.fn() }))
vi.mock('@/shared/services/browser-session-service', () => ({
  resizeBrowserSession: (...args: unknown[]) => mocks.resize(...args),
  navigateBrowserSession: vi.fn(),
}))
vi.mock('../RemoteBrowserCanvas', () => ({
  default: ({ statusText }: { statusText: string }) => <div role="status">{statusText}</div>,
}))
vi.mock('../useRemoteBrowserStream', () => ({ useRemoteBrowserStream: () => vi.fn() }))
vi.mock('../useRemoteBrowserInteraction', () => ({
  useRemoteBrowserInteraction: () => ({ sinkRef: { current: null }, updatePointer: vi.fn() }),
}))

const session = {
  id: 'resize-session',
  viewport: { width: 800, height: 600 },
  device_scale_factor: 1,
  url: 'about:blank',
} as BrowserSession
let observe: ResizeObserverCallback
const changeSize = (width: number) =>
  observe([{ contentRect: { width, height: 700 } } as ResizeObserverEntry], {} as ResizeObserver)

describe('RemoteBrowserPanel 视口同步', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mocks.resize.mockReset().mockResolvedValue({ ok: true })
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: ResizeObserverCallback) {
          observe = callback
        }
        observe() {}
        disconnect() {}
      }
    )
  })
  afterEach(() => {
    cleanup()
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it('同一会话只发一个尺寸请求，合并期间的变化并忽略重复尺寸', async () => {
    let finish!: (value: unknown) => void
    mocks.resize.mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)))
    render(<RemoteBrowserPanel session={session} mode="login" />)
    await act(async () => {
      changeSize(900)
      await vi.advanceTimersByTimeAsync(200)
    })
    await act(async () => {
      changeSize(1000)
      await vi.advanceTimersByTimeAsync(200)
      changeSize(1200)
      await vi.advanceTimersByTimeAsync(200)
    })
    expect(mocks.resize).toHaveBeenCalledTimes(1)
    await act(async () => finish({ ok: true }))
    expect(mocks.resize).toHaveBeenCalledTimes(2)
    expect(mocks.resize).toHaveBeenLastCalledWith('resize-session', 1200, 700, 1)
    await act(async () => {
      changeSize(1200)
      await vi.advanceTimersByTimeAsync(200)
    })
    expect(mocks.resize).toHaveBeenCalledTimes(2)
    expect(screen.getByRole('status')).toHaveTextContent('远程视口尺寸已同步')
  })

  it('CDP 失败显示在画面状态中，后续尺寸变化可继续重试', async () => {
    mocks.resize.mockRejectedValueOnce(new Error('cdp error: 视口请求超时'))
    render(<RemoteBrowserPanel session={session} mode="login" />)
    await act(async () => {
      changeSize(900)
      await vi.advanceTimersByTimeAsync(200)
    })
    expect(screen.getByRole('status')).toHaveTextContent('cdp error: 视口请求超时')
    await act(async () => {
      changeSize(900)
      await vi.advanceTimersByTimeAsync(200)
    })
    expect(mocks.resize).toHaveBeenCalledTimes(2)
  })

  it('离开页面后取消尚未发送的尺寸同步', async () => {
    const view = render(<RemoteBrowserPanel session={session} mode="login" />)
    act(() => changeSize(900))
    view.unmount()
    await act(async () => vi.advanceTimersByTimeAsync(200))
    expect(mocks.resize).not.toHaveBeenCalled()
  })
})

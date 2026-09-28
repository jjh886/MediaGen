import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useRemoteBrowserStream } from '../useRemoteBrowserStream'
import { HttpApiError } from '@/shared/services/http-api-client'

const mocks = vi.hoisted(() => ({ sendFallback: vi.fn() }))

vi.mock('@/shared/services/browser-session-service', async () => {
  const actual = await vi.importActual<object>('@/shared/services/browser-session-service')
  return {
    ...actual,
    browserSessionStreamUrl: () => 'ws://localhost/api/browser-sessions/session-1/stream',
    sendBrowserInput: (...args: unknown[]) => mocks.sendFallback(...args),
  }
})

class FakeWebSocket {
  static OPEN = 1
  static instances: FakeWebSocket[] = []
  readyState = FakeWebSocket.OPEN
  binaryType = ''
  sent: string[] = []
  onopen: (() => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onclose: (() => void) | null = null
  onerror: (() => void) | null = null

  constructor(public url: string) {
    FakeWebSocket.instances.push(this)
  }

  send(value: string) {
    this.sent.push(value)
  }
  close() {
    this.readyState = 3
  }
}

function Harness({
  onFrame,
  onError = vi.fn(),
}: {
  onFrame: (blob: Blob) => void
  onError?: (message: string) => void
}) {
  const [cursor, setCursor] = useState('')
  const send = useRemoteBrowserStream('session-1', {
    onFrame: (blob) => onFrame(blob),
    onSession: vi.fn(),
    onInput: (result) => setCursor(result.cursor),
    onConnection: vi.fn(),
    onError,
  })
  return (
    <button onClick={() => send({ kind: 'mouse', event: 'move', x: 1, y: 2 })}>
      {cursor || '发送'}
    </button>
  )
}

describe('useRemoteBrowserStream', () => {
  beforeEach(() => {
    FakeWebSocket.instances = []
    mocks.sendFallback.mockReset()
    vi.stubGlobal('WebSocket', FakeWebSocket)
  })

  it('reports input errors from the stream without exposing server details', () => {
    const onError = vi.fn()
    render(<Harness onFrame={vi.fn()} onError={onError} />)
    const socket = FakeWebSocket.instances[0]
    act(() =>
      socket.onmessage?.(
        new MessageEvent('message', {
          data: JSON.stringify({ type: 'error', sequence: 1, error: 'private server details' }),
        })
      )
    )
    expect(onError).toHaveBeenCalledExactlyOnceWith('远程输入未完成，请检查输入框内容后重试')
  })

  it.each([408, 502])(
    'reports HTTP %s from fallback input without replaying it',
    async (status) => {
      const onError = vi.fn()
      mocks.sendFallback.mockRejectedValue(new HttpApiError('private server details', status))
      render(<Harness onFrame={vi.fn()} onError={onError} />)
      FakeWebSocket.instances[0].readyState = 3
      fireEvent.click(screen.getByText('发送'))

      await waitFor(() =>
        expect(onError).toHaveBeenCalledExactlyOnceWith(
          status === 408
            ? '远程输入请求超时，请检查输入框内容后重试'
            : '远程输入未完成，请检查输入框内容后重试'
        )
      )
      expect(mocks.sendFallback).toHaveBeenCalledTimes(1)
    }
  )

  it('reports a socket send failure without falling back and duplicating input', () => {
    const onError = vi.fn()
    render(<Harness onFrame={vi.fn()} onError={onError} />)
    vi.spyOn(FakeWebSocket.instances[0], 'send').mockImplementation(() => {
      throw new Error('socket unavailable')
    })
    fireEvent.click(screen.getByText('发送'))
    expect(onError).toHaveBeenCalledExactlyOnceWith('远程输入未完成，请检查输入框内容后重试')
    expect(mocks.sendFallback).not.toHaveBeenCalled()
  })

  afterEach(() => vi.unstubAllGlobals())

  it('keeps input ordered and receives binary frames', () => {
    const onFrame = vi.fn()
    render(<Harness onFrame={onFrame} />)
    const socket = FakeWebSocket.instances[0]
    fireEvent.click(screen.getByText('发送'))

    expect(JSON.parse(socket.sent[0])).toMatchObject({ type: 'input', sequence: 1 })
    act(() => {
      socket.onmessage?.(
        new MessageEvent('message', {
          data: JSON.stringify({
            type: 'input_result',
            sequence: 1,
            data: { ok: true, cursor: 'text', pointer: { cursor: 'text' } },
          }),
        })
      )
    })
    expect(screen.getByText('text')).toBeInTheDocument()

    const frame = new Blob(['jpeg'], { type: 'image/jpeg' })
    act(() => {
      socket.onmessage?.(
        new MessageEvent('message', {
          data: JSON.stringify({
            type: 'frame',
            sequence: 2,
            data: { viewport: { width: 800, height: 600 } },
          }),
        })
      )
      socket.onmessage?.(new MessageEvent('message', { data: frame }))
    })
    expect(onFrame).toHaveBeenCalledWith(frame)
  })
})

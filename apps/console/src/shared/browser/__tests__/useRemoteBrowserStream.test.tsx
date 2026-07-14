import { act, fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useRemoteBrowserStream } from '../useRemoteBrowserStream'

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

function Harness({ onFrame }: { onFrame: (blob: Blob) => void }) {
  const [cursor, setCursor] = useState('')
  const send = useRemoteBrowserStream('session-1', {
    onFrame: (blob) => onFrame(blob),
    onSession: vi.fn(),
    onInput: (result) => setCursor(result.cursor),
    onConnection: vi.fn(),
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
    vi.stubGlobal('WebSocket', FakeWebSocket)
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

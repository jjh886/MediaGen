import { useEffect, useRef } from 'react'
import {
  browserSessionStreamUrl,
  sendBrowserInput,
  type BrowserInputRequest,
  type BrowserInputResponse,
  type BrowserSession,
} from '@/shared/services/browser-session-service'
import { HttpApiError } from '@/shared/services/http-api-client'

interface FrameMetadata {
  viewport?: BrowserSession['viewport']
  device_scale_factor?: number
}

interface StreamCallbacks {
  onFrame: (blob: Blob, metadata: FrameMetadata) => void
  onSession: (session: BrowserSession) => void
  onInput: (result: BrowserInputResponse) => void
  onConnection: (connected: boolean) => void
  onError: (message: string) => void
}

export function useRemoteBrowserStream(
  sessionId: string,
  callbacks: StreamCallbacks
): (input: BrowserInputRequest) => void {
  const socketRef = useRef<WebSocket | null>(null)
  const sequenceRef = useRef(0)
  const frameMetadataRef = useRef<FrameMetadata>({})
  const callbacksRef = useRef(callbacks)
  callbacksRef.current = callbacks

  useEffect(() => {
    let closed = false
    let reconnectTimer: number | undefined
    let reconnectDelay = 250

    const connect = () => {
      const socket = new WebSocket(browserSessionStreamUrl(sessionId))
      socket.binaryType = 'blob'
      socketRef.current = socket
      socket.onopen = () => {
        reconnectDelay = 250
        callbacksRef.current.onConnection(true)
      }
      socket.onmessage = (event) => {
        if (event.data instanceof Blob) {
          callbacksRef.current.onFrame(event.data, frameMetadataRef.current)
          return
        }
        handleTextMessage(String(event.data), callbacksRef.current, frameMetadataRef)
      }
      socket.onclose = () => {
        callbacksRef.current.onConnection(false)
        if (closed) return
        reconnectTimer = window.setTimeout(connect, reconnectDelay)
        reconnectDelay = Math.min(4000, reconnectDelay * 2)
      }
      socket.onerror = () => socket.close()
    }

    connect()
    return () => {
      closed = true
      if (reconnectTimer) window.clearTimeout(reconnectTimer)
      socketRef.current?.close()
      socketRef.current = null
    }
  }, [sessionId])

  return (input) => {
    const sequence = ++sequenceRef.current
    const socket = socketRef.current
    if (socket?.readyState === WebSocket.OPEN) {
      try {
        socket.send(JSON.stringify({ type: 'input', sequence, input }))
      } catch {
        callbacksRef.current.onError('远程输入未完成，请检查输入框内容后重试')
      }
      return
    }
    void sendBrowserInput(sessionId, input)
      .then(callbacksRef.current.onInput)
      .catch((error: unknown) =>
        callbacksRef.current.onError(
          error instanceof HttpApiError && error.status === 408
            ? '远程输入请求超时，请检查输入框内容后重试'
            : '远程输入未完成，请检查输入框内容后重试'
        )
      )
  }
}

function handleTextMessage(
  text: string,
  callbacks: StreamCallbacks,
  frameMetadataRef: { current: FrameMetadata }
) {
  const message = JSON.parse(text) as {
    type?: string
    event?: string
    data?: BrowserInputResponse | BrowserSession | FrameMetadata
  }
  if (message.type === 'frame') {
    frameMetadataRef.current = (message.data as FrameMetadata) || {}
  } else if (message.type === 'input_result') {
    callbacks.onInput(message.data as BrowserInputResponse)
  } else if (message.type === 'event' && message.event === 'status') {
    callbacks.onSession(message.data as BrowserSession)
  } else if (message.type === 'error') {
    callbacks.onError('远程输入未完成，请检查输入框内容后重试')
  }
}

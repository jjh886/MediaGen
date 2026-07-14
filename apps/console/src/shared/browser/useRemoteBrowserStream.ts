import { useEffect, useRef } from 'react'
import {
  browserSessionStreamUrl,
  sendBrowserInput,
  type BrowserInputRequest,
  type BrowserInputResponse,
  type BrowserSession,
} from '@/shared/services/browser-session-service'

interface FrameMetadata {
  viewport?: BrowserSession['viewport']
  device_scale_factor?: number
}

interface StreamCallbacks {
  onFrame: (blob: Blob, metadata: FrameMetadata) => void
  onSession: (session: BrowserSession) => void
  onInput: (result: BrowserInputResponse) => void
  onConnection: (connected: boolean) => void
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
      socket.send(JSON.stringify({ type: 'input', sequence, input }))
      return
    }
    void sendBrowserInput(sessionId, input)
      .then(callbacksRef.current.onInput)
      .catch(() => undefined)
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
  }
}

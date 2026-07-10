import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type PointerEvent,
  type WheelEvent,
} from 'react'
import {
  browserSessionEventsUrl,
  copyBrowserSelection,
  navigateBrowserSession,
  resizeBrowserSession,
  sendBrowserInput,
  writeBrowserClipboard,
  type BrowserInputRequest,
  type BrowserSession,
} from '@/shared/services/browser-session-service'
import { KtButton } from '@/shared/ui'

export interface RemoteBrowserPanelProps {
  session: BrowserSession
  mode?: 'login' | 'debug'
}

function buttonName(button: number): string {
  if (button === 1) return 'middle'
  if (button === 2) return 'right'
  return 'left'
}

function buttonMask(button: number): number {
  if (button === 1) return 4
  if (button === 2) return 2
  return 1
}

function modifiers(
  event: Pick<
    KeyboardEvent | PointerEvent | WheelEvent,
    'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey'
  >
) {
  return (
    (event.altKey ? 1 : 0) |
    (event.ctrlKey ? 2 : 0) |
    (event.metaKey ? 4 : 0) |
    (event.shiftKey ? 8 : 0)
  )
}

function browserDeviceScaleFactor(): number {
  return Math.min(2, Math.max(1, window.devicePixelRatio || 1))
}

export default function RemoteBrowserPanel({ session, mode = 'debug' }: RemoteBrowserPanelProps) {
  const showTools = mode === 'debug'
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const stageRef = useRef<HTMLDivElement | null>(null)
  const resizeTimerRef = useRef<number | null>(null)
  const moveFrameRef = useRef<number | null>(null)
  const queuedMoveRef = useRef<BrowserInputRequest | null>(null)
  const lastCursorReadAtRef = useRef(0)
  const frameSerialRef = useRef(0)
  const pointerDownRef = useRef(false)
  const urlInputRef = useRef<HTMLInputElement | null>(null)
  const [viewport, setViewport] = useState(session.viewport)
  const [deviceScaleFactor, setDeviceScaleFactor] = useState(
    session.device_scale_factor || browserDeviceScaleFactor()
  )
  const [statusText, setStatusText] = useState('正在连接远程浏览器')
  const [currentUrl, setCurrentUrl] = useState(session.url || session.start_url)
  const [urlInput, setUrlInput] = useState(session.url || session.start_url)
  const [pasteText, setPasteText] = useState('')
  const [remoteCursor, setRemoteCursor] = useState('default')
  const [hasFrame, setHasFrame] = useState(false)

  const stageStyle = useMemo(() => {
    if (!showTools) return {}
    const aspect = viewport.width / viewport.height
    return { aspectRatio: `${aspect}` }
  }, [showTools, viewport.height, viewport.width])

  useEffect(() => {
    return () => {
      if (moveFrameRef.current) window.cancelAnimationFrame(moveFrameRef.current)
    }
  }, [])

  useEffect(() => {
    const events = new EventSource(browserSessionEventsUrl(session.id))
    const updateFromSession = (next: BrowserSession) => {
      const nextUrl = next.url || next.start_url || 'about:blank'
      setViewport(next.viewport)
      setDeviceScaleFactor(next.device_scale_factor || browserDeviceScaleFactor())
      setCurrentUrl(nextUrl)
      if (document.activeElement !== urlInputRef.current) setUrlInput(nextUrl)
      setStatusText(next.status === 'active' ? '远程浏览器已连接' : `会话状态：${next.status}`)
    }

    events.addEventListener('status', (event) => {
      updateFromSession(JSON.parse(event.data) as BrowserSession)
    })
    events.addEventListener('frame', (event) => {
      const data = JSON.parse(event.data) as {
        image?: string
        viewport?: typeof viewport
        device_scale_factor?: number
      }
      if (data.viewport) setViewport(data.viewport)
      if (data.device_scale_factor) setDeviceScaleFactor(data.device_scale_factor)
      if (data.image) drawFrame(data.image)
    })
    events.onerror = () => setStatusText('连接中断，正在等待重连')

    return () => events.close()
  }, [session.id])

  useEffect(() => {
    if (showTools) return
    const stage = stageRef.current
    if (!stage) return
    // 登录弹窗可被用户拉伸，容器变化后同步远端浏览器视口，避免画面被压缩。
    const observer = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect
      if (!rect) return
      const width = Math.round(rect.width)
      const height = Math.round(rect.height)
      const scale = browserDeviceScaleFactor()
      if (width < 480 || height < 320) return
      setViewport((prev) =>
        prev.width === width && prev.height === height ? prev : { width, height }
      )
      setDeviceScaleFactor(scale)
      if (resizeTimerRef.current) window.clearTimeout(resizeTimerRef.current)
      resizeTimerRef.current = window.setTimeout(() => {
        resizeBrowserSession(session.id, width, height, scale).catch(() => undefined)
      }, 180)
    })
    observer.observe(stage)
    return () => {
      observer.disconnect()
      if (resizeTimerRef.current) window.clearTimeout(resizeTimerRef.current)
      if (moveFrameRef.current) window.cancelAnimationFrame(moveFrameRef.current)
    }
  }, [session.id, showTools])

  const drawFrame = (base64: string) => {
    const canvas = canvasRef.current
    if (!canvas) return
    const context = canvas.getContext('2d')
    if (!context) return
    const serial = ++frameSerialRef.current
    const image = new Image()
    image.onload = () => {
      if (serial !== frameSerialRef.current) return
      const width = image.naturalWidth || Math.round(viewport.width * deviceScaleFactor)
      const height = image.naturalHeight || Math.round(viewport.height * deviceScaleFactor)
      if (canvas.width !== width) canvas.width = width
      if (canvas.height !== height) canvas.height = height
      context.imageSmoothingEnabled = true
      context.imageSmoothingQuality = 'high'
      context.drawImage(image, 0, 0, canvas.width, canvas.height)
      setHasFrame(true)
    }
    image.src = `data:image/jpeg;base64,${base64}`
  }

  const pointFromEvent = (
    event: PointerEvent<HTMLCanvasElement> | WheelEvent<HTMLCanvasElement>
  ) => {
    const rect = event.currentTarget.getBoundingClientRect()
    return {
      x: ((event.clientX - rect.left) / rect.width) * viewport.width,
      y: ((event.clientY - rect.top) / rect.height) * viewport.height,
    }
  }

  const sendTextToBrowser = async (text: string) => {
    if (!text) return
    await writeBrowserClipboard(session.id, text)
    canvasRef.current?.focus()
  }

  const sendInputFast = (input: BrowserInputRequest) => {
    void sendBrowserInput(session.id, input)
      .then((result) => {
        if (result.cursor) setRemoteCursor(result.cursor)
      })
      .catch(() => undefined)
  }

  const queueMouseMove = (input: BrowserInputRequest) => {
    queuedMoveRef.current = input
    if (moveFrameRef.current) return
    moveFrameRef.current = window.requestAnimationFrame(() => {
      moveFrameRef.current = null
      const next = queuedMoveRef.current
      queuedMoveRef.current = null
      if (next) sendInputFast(next)
    })
  }

  const copyRemoteText = async () => {
    const text = await copyBrowserSelection(session.id)
    if (!text) return
    await navigator.clipboard.writeText(text).catch(() => undefined)
    setStatusText('已复制远端选中文本')
    canvasRef.current?.focus()
  }

  const handleNavigate = async (event: FormEvent) => {
    event.preventDefault()
    await navigateBrowserSession(session.id, urlInput)
    canvasRef.current?.focus()
  }

  const handlePasteShortcut = async (event: KeyboardEvent<HTMLCanvasElement>) => {
    const key = event.key.toLowerCase()
    if (!(event.metaKey || event.ctrlKey)) return false
    if (key === 'c') {
      event.preventDefault()
      await copyRemoteText()
      return true
    }
    if (key === 'v') {
      event.preventDefault()
      const text = await navigator.clipboard.readText().catch(() => '')
      await sendTextToBrowser(text)
      return true
    }
    return false
  }

  return (
    <div
      style={{
        height: '100%',
        minHeight: 0,
        display: 'flex',
        flexDirection: 'column',
        gap: showTools ? 12 : 0,
      }}
    >
      {showTools && (
        <>
          <form
            onSubmit={handleNavigate}
            style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) auto', gap: 8 }}
          >
            <input
              ref={urlInputRef}
              value={urlInput}
              onChange={(event) => setUrlInput(event.target.value)}
              style={{
                minWidth: 0,
                height: 34,
                padding: '0 10px',
                border: '1px solid var(--border-base)',
                borderRadius: 6,
                background: 'var(--input-bg)',
                color: 'var(--text-primary)',
              }}
            />
            <KtButton variant="default" type="submit">
              跳转
            </KtButton>
          </form>

          <div
            style={{
              display: 'grid',
              gridTemplateColumns: '1fr auto',
              gap: 8,
              alignItems: 'center',
              color: 'var(--text-secondary)',
              fontSize: 12,
            }}
          >
            <div style={{ minWidth: 0, overflowWrap: 'anywhere' }}>
              {currentUrl || 'about:blank'}
            </div>
            <div>{statusText}</div>
          </div>
        </>
      )}

      <div
        style={
          showTools
            ? { display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) 240px', gap: 12 }
            : { flex: 1, minHeight: 0, display: 'flex' }
        }
      >
        <div
          ref={stageRef}
          style={{
            ...stageStyle,
            position: 'relative',
            flex: showTools ? undefined : 1,
            minHeight: 0,
            width: '100%',
            overflow: 'hidden',
            border: showTools ? '1px solid var(--border-base)' : 'none',
            background: '#f6f7f8',
          }}
        >
          <canvas
            ref={canvasRef}
            tabIndex={0}
            width={Math.round(viewport.width * deviceScaleFactor)}
            height={Math.round(viewport.height * deviceScaleFactor)}
            style={{
              display: 'block',
              width: '100%',
              height: '100%',
              cursor: remoteCursor || 'default',
              background: '#fff',
            }}
            onPointerDown={async (event) => {
              const point = pointFromEvent(event)
              pointerDownRef.current = true
              event.currentTarget.setPointerCapture(event.pointerId)
              event.currentTarget.focus()
              const result = await sendBrowserInput(session.id, {
                kind: 'mouse',
                event: 'down',
                ...point,
                button: buttonName(event.button),
                buttons: event.buttons || buttonMask(event.button),
                clickCount: event.detail || 1,
                modifiers: modifiers(event),
              })
              if (result.cursor) setRemoteCursor(result.cursor)
            }}
            onPointerMove={async (event) => {
              const point = pointFromEvent(event)
              const now = Date.now()
              const shouldReadCursor =
                !pointerDownRef.current && now - lastCursorReadAtRef.current > 120
              if (shouldReadCursor) lastCursorReadAtRef.current = now
              queueMouseMove({
                kind: 'mouse',
                event: 'move',
                ...point,
                button: pointerDownRef.current ? buttonName(event.button) : 'none',
                buttons: pointerDownRef.current ? event.buttons || 1 : 0,
                modifiers: modifiers(event),
                readCursor: shouldReadCursor,
              })
            }}
            onPointerUp={async (event) => {
              const point = pointFromEvent(event)
              pointerDownRef.current = false
              const result = await sendBrowserInput(session.id, {
                kind: 'mouse',
                event: 'up',
                ...point,
                button: buttonName(event.button),
                buttons: 0,
                clickCount: event.detail || 1,
                modifiers: modifiers(event),
              })
              if (result.cursor) setRemoteCursor(result.cursor)
            }}
            onWheel={(event) => {
              event.preventDefault()
              const point = pointFromEvent(event)
              sendInputFast({
                kind: 'wheel',
                ...point,
                deltaX: event.deltaX,
                deltaY: event.deltaY,
                modifiers: modifiers(event),
                readCursor: false,
              })
            }}
            onKeyDown={async (event) => {
              if (await handlePasteShortcut(event)) return
              event.preventDefault()
              await sendBrowserInput(session.id, {
                kind: 'key',
                event: 'down',
                key: event.key,
                code: event.code,
                ctrlKey: event.ctrlKey,
                metaKey: event.metaKey,
                altKey: event.altKey,
                modifiers: modifiers(event),
              })
            }}
            onKeyUp={async (event) => {
              if ((event.metaKey || event.ctrlKey) && ['c', 'v'].includes(event.key.toLowerCase()))
                return
              event.preventDefault()
              await sendBrowserInput(session.id, {
                kind: 'key',
                event: 'up',
                key: event.key,
                code: event.code,
                ctrlKey: event.ctrlKey,
                metaKey: event.metaKey,
                altKey: event.altKey,
                modifiers: modifiers(event),
              })
            }}
            onPaste={async (event) => {
              const text = event.clipboardData.getData('text')
              event.preventDefault()
              await sendTextToBrowser(text)
            }}
          />
          {!hasFrame && (
            <div
              style={{
                position: 'absolute',
                inset: 0,
                display: 'grid',
                placeItems: 'center',
                background: 'rgba(255,255,255,0.92)',
                color: '#30343b',
                fontSize: 13,
              }}
            >
              {statusText || '等待浏览器画面'}
            </div>
          )}
        </div>

        {showTools && (
          <div style={{ display: 'grid', alignContent: 'start', gap: 10 }}>
            <textarea
              rows={6}
              value={pasteText}
              placeholder="粘贴账号、验证码或多行文本"
              onChange={(event) => setPasteText(event.target.value)}
            />
            <KtButton variant="primary" onClick={() => sendTextToBrowser(pasteText)}>
              写入当前焦点
            </KtButton>
            <KtButton variant="default" onClick={copyRemoteText}>
              复制远端选中
            </KtButton>
            <KtButton
              variant="default"
              onClick={() =>
                resizeBrowserSession(session.id, viewport.width, viewport.height, deviceScaleFactor)
              }
            >
              同步视口尺寸
            </KtButton>
          </div>
        )}
      </div>
    </div>
  )
}

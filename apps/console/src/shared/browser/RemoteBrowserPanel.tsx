import { useEffect, useRef, useState, type FormEvent, type RefObject } from 'react'
import {
  navigateBrowserSession,
  resizeBrowserSession,
  type BrowserInputResponse,
  type BrowserSession,
} from '@/shared/services/browser-session-service'
import { KtButton } from '@/shared/ui'
import RemoteBrowserCanvas from './RemoteBrowserCanvas'
import { useRemoteBrowserInteraction } from './useRemoteBrowserInteraction'
import { useRemoteBrowserStream } from './useRemoteBrowserStream'

export interface RemoteBrowserPanelProps {
  session: BrowserSession
  mode?: 'login' | 'debug'
}

function browserDeviceScaleFactor(): number {
  return Math.min(2, Math.max(1, window.devicePixelRatio || 1))
}

export default function RemoteBrowserPanel({ session, mode = 'debug' }: RemoteBrowserPanelProps) {
  const showTools = mode === 'debug'
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const stageRef = useRef<HTMLDivElement | null>(null)
  const urlInputRef = useRef<HTMLInputElement | null>(null)
  const resizeTimerRef = useRef<number | null>(null)
  const frameSerialRef = useRef(0)
  const pointerUpdateRef = useRef<(result: BrowserInputResponse) => void>(() => undefined)
  const [viewport, setViewport] = useState(session.viewport)
  const [deviceScaleFactor, setDeviceScaleFactor] = useState(
    session.device_scale_factor || browserDeviceScaleFactor()
  )
  const [statusText, setStatusText] = useState('正在连接远程浏览器')
  const [currentUrl, setCurrentUrl] = useState(session.url || session.start_url)
  const [urlInput, setUrlInput] = useState(session.url || session.start_url)
  const [pasteText, setPasteText] = useState('')
  const [hasFrame, setHasFrame] = useState(false)

  const drawFrame = async (
    blob: Blob,
    metadata: { viewport?: BrowserSession['viewport']; device_scale_factor?: number }
  ) => {
    const serial = ++frameSerialRef.current
    const bitmap = await createImageBitmap(blob)
    if (serial !== frameSerialRef.current) {
      bitmap.close()
      return
    }
    const canvas = canvasRef.current
    const context = canvas?.getContext('2d')
    if (!canvas || !context) {
      bitmap.close()
      return
    }
    if (metadata.viewport) setViewport(metadata.viewport)
    if (metadata.device_scale_factor) setDeviceScaleFactor(metadata.device_scale_factor)
    if (canvas.width !== bitmap.width) canvas.width = bitmap.width
    if (canvas.height !== bitmap.height) canvas.height = bitmap.height
    context.imageSmoothingEnabled = true
    context.imageSmoothingQuality = 'high'
    context.drawImage(bitmap, 0, 0)
    bitmap.close()
    setHasFrame(true)
  }

  const updateFromSession = (next: BrowserSession) => {
    const nextUrl = next.url || next.start_url || 'about:blank'
    setViewport(next.viewport)
    setDeviceScaleFactor(next.device_scale_factor || browserDeviceScaleFactor())
    setCurrentUrl(nextUrl)
    if (document.activeElement !== urlInputRef.current) setUrlInput(nextUrl)
    setStatusText(next.status === 'active' ? '远程浏览器已连接' : `会话状态：${next.status}`)
  }

  const sendInput = useRemoteBrowserStream(session.id, {
    onFrame: (blob, metadata) => void drawFrame(blob, metadata),
    onSession: updateFromSession,
    onInput: (result) => pointerUpdateRef.current(result),
    onConnection: (connected) =>
      setStatusText(connected ? '远程浏览器已连接' : '连接中断，正在等待重连'),
  })
  const interaction = useRemoteBrowserInteraction({
    sessionId: session.id,
    viewport,
    sendInput,
    onStatus: setStatusText,
  })
  pointerUpdateRef.current = (result) => interaction.updatePointer(result.pointer)

  useEffect(() => {
    if (showTools) return
    const stage = stageRef.current
    if (!stage) return
    const observer = new ResizeObserver(([entry]) => {
      const width = Math.round(entry.contentRect.width)
      const height = Math.round(entry.contentRect.height)
      const scale = browserDeviceScaleFactor()
      if (width < 480 || height < 320) return
      setViewport((current) =>
        current.width === width && current.height === height ? current : { width, height }
      )
      setDeviceScaleFactor(scale)
      if (resizeTimerRef.current) window.clearTimeout(resizeTimerRef.current)
      resizeTimerRef.current = window.setTimeout(() => {
        void resizeBrowserSession(session.id, width, height, scale)
      }, 180)
    })
    observer.observe(stage)
    return () => {
      observer.disconnect()
      if (resizeTimerRef.current) window.clearTimeout(resizeTimerRef.current)
    }
  }, [session.id, showTools])

  const handleNavigate = async (event: FormEvent) => {
    event.preventDefault()
    await navigateBrowserSession(session.id, urlInput)
    interaction.sinkRef.current?.focus()
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
      {showTools ? (
        <DebugAddressBar
          currentUrl={currentUrl}
          statusText={statusText}
          urlInput={urlInput}
          urlInputRef={urlInputRef}
          onUrlChange={setUrlInput}
          onNavigate={handleNavigate}
        />
      ) : null}
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
            position: 'relative',
            flex: showTools ? undefined : 1,
            minHeight: 0,
            width: '100%',
            aspectRatio: showTools ? `${viewport.width / viewport.height}` : undefined,
            overflow: 'hidden',
            border: showTools ? '1px solid var(--border-base)' : 'none',
            background: '#f6f7f8',
          }}
        >
          <RemoteBrowserCanvas
            canvasRef={canvasRef}
            sinkRef={interaction.sinkRef}
            viewport={viewport}
            deviceScaleFactor={deviceScaleFactor}
            pointer={interaction.pointer}
            pointerHandlers={interaction.pointerHandlers}
            keyHandlers={interaction.keyHandlers}
            menu={interaction.menu}
            onMenuClose={() => interaction.setMenu(null)}
            onMenuAction={interaction.runContextAction}
            hasFrame={hasFrame}
            statusText={statusText}
          />
        </div>
        {showTools ? (
          <DebugInputTools
            text={pasteText}
            onTextChange={setPasteText}
            onWrite={() => void interaction.sendText(pasteText)}
            onCopy={() => void interaction.copy()}
            onResize={() =>
              void resizeBrowserSession(
                session.id,
                viewport.width,
                viewport.height,
                deviceScaleFactor
              )
            }
          />
        ) : null}
      </div>
    </div>
  )
}

interface AddressBarProps {
  currentUrl: string
  statusText: string
  urlInput: string
  urlInputRef: RefObject<HTMLInputElement | null>
  onUrlChange: (value: string) => void
  onNavigate: (event: FormEvent) => void
}

function DebugAddressBar(props: AddressBarProps) {
  return (
    <>
      <form
        onSubmit={props.onNavigate}
        style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) auto', gap: 8 }}
      >
        <input
          ref={props.urlInputRef}
          value={props.urlInput}
          onChange={(event) => props.onUrlChange(event.target.value)}
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
          {props.currentUrl || 'about:blank'}
        </div>
        <div>{props.statusText}</div>
      </div>
    </>
  )
}

interface InputToolsProps {
  text: string
  onTextChange: (value: string) => void
  onWrite: () => void
  onCopy: () => void
  onResize: () => void
}

function DebugInputTools(props: InputToolsProps) {
  return (
    <div style={{ display: 'grid', alignContent: 'start', gap: 10 }}>
      <textarea
        rows={6}
        value={props.text}
        placeholder="粘贴账号、验证码或多行文本"
        onChange={(event) => props.onTextChange(event.target.value)}
      />
      <KtButton variant="primary" onClick={props.onWrite}>
        写入当前焦点
      </KtButton>
      <KtButton variant="default" onClick={props.onCopy}>
        复制远端选中
      </KtButton>
      <KtButton variant="default" onClick={props.onResize}>
        同步视口尺寸
      </KtButton>
    </div>
  )
}

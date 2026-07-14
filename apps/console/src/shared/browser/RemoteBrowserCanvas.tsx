import { type CanvasHTMLAttributes, type RefObject, type TextareaHTMLAttributes } from 'react'
import type { BrowserPointerState, BrowserSession } from '@/shared/services/browser-session-service'
import RemoteBrowserContextMenu, { type ContextMenuPosition } from './RemoteBrowserContextMenu'
import { safeRemoteCursor } from './remote-browser-cursor'

interface RemoteBrowserCanvasProps {
  canvasRef: RefObject<HTMLCanvasElement | null>
  sinkRef: RefObject<HTMLTextAreaElement | null>
  viewport: BrowserSession['viewport']
  deviceScaleFactor: number
  pointer: BrowserPointerState
  pointerHandlers: CanvasHTMLAttributes<HTMLCanvasElement>
  keyHandlers: TextareaHTMLAttributes<HTMLTextAreaElement>
  menu: ContextMenuPosition | null
  onMenuClose: () => void
  onMenuAction: (action: string) => void
  hasFrame: boolean
  statusText: string
}

export default function RemoteBrowserCanvas(props: RemoteBrowserCanvasProps) {
  return (
    <>
      <canvas
        ref={props.canvasRef}
        width={Math.round(props.viewport.width * props.deviceScaleFactor)}
        height={Math.round(props.viewport.height * props.deviceScaleFactor)}
        style={{
          display: 'block',
          width: '100%',
          height: '100%',
          cursor: safeRemoteCursor(props.pointer.cursor),
          background: '#fff',
        }}
        {...props.pointerHandlers}
      />
      <textarea
        ref={props.sinkRef}
        aria-label="远程网页输入"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        style={{
          position: 'absolute',
          width: 1,
          height: 1,
          left: -2,
          top: -2,
          padding: 0,
          opacity: 0,
          pointerEvents: 'none',
        }}
        {...props.keyHandlers}
      />
      {!props.hasFrame ? (
        <div
          style={{
            position: 'absolute',
            inset: 0,
            display: 'grid',
            placeItems: 'center',
            background: 'rgba(255,255,255,0.92)',
            color: '#30343b',
            fontSize: 13,
            pointerEvents: 'none',
          }}
        >
          {props.statusText || '等待浏览器画面'}
        </div>
      ) : null}
      {props.menu ? (
        <RemoteBrowserContextMenu
          position={props.menu}
          canCopy={!props.pointer.password && props.pointer.has_selection}
          canEdit={props.pointer.editable}
          onAction={props.onMenuAction}
          onClose={props.onMenuClose}
        />
      ) : null}
    </>
  )
}

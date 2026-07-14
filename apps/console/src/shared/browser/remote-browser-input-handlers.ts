import type {
  ClipboardEvent,
  CompositionEvent,
  KeyboardEvent,
  MouseEvent,
  MutableRefObject,
  PointerEvent,
  WheelEvent,
} from 'react'
import type { BrowserInputRequest } from '@/shared/services/browser-session-service'
import { editingCommands, remoteShortcut } from './remote-browser-keymap'
import type { ContextMenuPosition } from './RemoteBrowserContextMenu'

interface InputHandlerOptions {
  viewport: { width: number; height: number }
  sendInput: (input: BrowserInputRequest) => void
}

interface PointerHandlerOptions extends InputHandlerOptions {
  focus: () => void
  setMenu: (menu: ContextMenuPosition | null) => void
}

export function createPointerHandlers(options: PointerHandlerOptions) {
  let pointerDown = false
  let lastCursorRead = 0
  let moveFrame: number | null = null
  let queuedMove: BrowserInputRequest | null = null

  const queueMove = (input: BrowserInputRequest) => {
    queuedMove = input
    if (moveFrame) return
    moveFrame = window.requestAnimationFrame(() => {
      moveFrame = null
      if (queuedMove) options.sendInput(queuedMove)
      queuedMove = null
    })
  }
  const point = (event: PointerEvent<HTMLCanvasElement> | WheelEvent<HTMLCanvasElement>) => {
    const rect = event.currentTarget.getBoundingClientRect()
    return {
      x: ((event.clientX - rect.left) / rect.width) * options.viewport.width,
      y: ((event.clientY - rect.top) / rect.height) * options.viewport.height,
    }
  }

  return {
    cancel: () => {
      if (moveFrame) window.cancelAnimationFrame(moveFrame)
    },
    handlers: {
      onPointerDown: (event: PointerEvent<HTMLCanvasElement>) => {
        options.setMenu(null)
        pointerDown = true
        event.currentTarget.setPointerCapture(event.pointerId)
        options.focus()
        options.sendInput(mouseInput('down', event, point(event), true))
      },
      onPointerMove: (event: PointerEvent<HTMLCanvasElement>) => {
        const now = performance.now()
        const readCursor = !pointerDown && now - lastCursorRead >= 50
        if (readCursor) lastCursorRead = now
        queueMove(mouseInput('move', event, point(event), readCursor))
      },
      onPointerUp: (event: PointerEvent<HTMLCanvasElement>) => {
        pointerDown = false
        options.sendInput(mouseInput('up', event, point(event), true))
      },
      onWheel: (event: WheelEvent<HTMLCanvasElement>) => {
        event.preventDefault()
        options.sendInput(wheelInput(event, point(event)))
      },
      onContextMenu: (event: MouseEvent<HTMLCanvasElement>) => {
        event.preventDefault()
        options.setMenu(clampMenuPosition(event.clientX, event.clientY))
      },
    },
  }
}

interface KeyboardHandlerOptions extends InputHandlerOptions {
  composingRef: MutableRefObject<boolean>
  runAction: (action: string) => void
  sendText: (text: string) => Promise<void>
}

export function createKeyboardHandlers(options: KeyboardHandlerOptions) {
  return {
    onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => {
      if (options.composingRef.current || event.nativeEvent.isComposing) return
      const shortcut = remoteShortcut(event)
      event.preventDefault()
      if (shortcut) return options.runAction(shortcut)
      options.sendInput(keyInput('down', event))
    },
    onKeyUp: (event: KeyboardEvent<HTMLTextAreaElement>) => {
      if (options.composingRef.current || event.nativeEvent.isComposing) return
      if (remoteShortcut(event)) return
      event.preventDefault()
      options.sendInput(keyInput('up', event))
      event.currentTarget.value = ''
    },
    onPaste: (event: ClipboardEvent<HTMLTextAreaElement>) => {
      event.preventDefault()
      void options.sendText(event.clipboardData.getData('text'))
    },
    onCompositionStart: () => {
      options.composingRef.current = true
    },
    onCompositionUpdate: (event: CompositionEvent<HTMLTextAreaElement>) => {
      options.sendInput(compositionInput('set_composition', event.data))
    },
    onCompositionEnd: (event: CompositionEvent<HTMLTextAreaElement>) => {
      options.sendInput(compositionInput('commit', event.data))
      options.composingRef.current = false
      event.currentTarget.value = ''
    },
  }
}

function mouseInput(
  eventType: 'move' | 'down' | 'up',
  event: PointerEvent<HTMLCanvasElement>,
  point: { x: number; y: number },
  readCursor: boolean
): BrowserInputRequest {
  return {
    kind: 'mouse',
    event: eventType,
    ...point,
    button: buttonName(event.button),
    buttons: eventType === 'up' ? 0 : event.buttons || buttonMask(event.button),
    clickCount: event.detail || 1,
    modifiers: modifiers(event),
    readCursor,
  }
}

function wheelInput(
  event: WheelEvent<HTMLCanvasElement>,
  point: { x: number; y: number }
): BrowserInputRequest {
  return {
    kind: 'wheel',
    ...point,
    deltaX: event.deltaX,
    deltaY: event.deltaY,
    modifiers: modifiers(event),
    readCursor: false,
  }
}

function keyInput(
  eventType: 'down' | 'up',
  event: KeyboardEvent<HTMLElement>
): BrowserInputRequest {
  return {
    kind: 'key',
    event: eventType,
    key: event.key,
    code: event.code,
    ctrlKey: event.ctrlKey,
    metaKey: event.metaKey,
    altKey: event.altKey,
    shiftKey: event.shiftKey,
    modifiers: modifiers(event),
    autoRepeat: event.repeat,
    isKeypad: event.location === 3,
    location: event.location,
    commands: eventType === 'down' ? editingCommands(event) : [],
  }
}

function compositionInput(event: 'set_composition' | 'commit', text: string): BrowserInputRequest {
  return {
    kind: 'ime',
    event,
    text,
    selectionStart: text.length,
    selectionEnd: text.length,
  }
}

function modifiers(event: {
  altKey: boolean
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
}) {
  return (
    (event.altKey ? 1 : 0) |
    (event.ctrlKey ? 2 : 0) |
    (event.metaKey ? 4 : 0) |
    (event.shiftKey ? 8 : 0)
  )
}

function buttonName(button: number) {
  return ['left', 'middle', 'right'][button] || 'left'
}

function buttonMask(button: number) {
  return [1, 4, 2][button] || 1
}

function clampMenuPosition(x: number, y: number): ContextMenuPosition {
  return { x: Math.min(x, window.innerWidth - 196), y: Math.min(y, window.innerHeight - 260) }
}

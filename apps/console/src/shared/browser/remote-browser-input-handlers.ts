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
  const moveQueue = createMoveQueue(options.sendInput)
  const point = (event: PointerEvent<HTMLCanvasElement> | WheelEvent<HTMLCanvasElement>) => {
    const rect = event.currentTarget.getBoundingClientRect()
    return {
      x: ((event.clientX - rect.left) / rect.width) * options.viewport.width,
      y: ((event.clientY - rect.top) / rect.height) * options.viewport.height,
    }
  }

  return {
    cancel: moveQueue.cancel,
    handlers: {
      onPointerDown: (event: PointerEvent<HTMLCanvasElement>) => {
        // 防止画布的默认点击行为覆盖隐藏输入接收器的焦点。
        event.preventDefault()
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
        moveQueue.push(mouseInput('move', event, point(event), readCursor))
      },
      onPointerUp: (event: PointerEvent<HTMLCanvasElement>) => {
        event.preventDefault()
        pointerDown = false
        options.sendInput(mouseInput('up', event, point(event), true))
        options.focus()
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

function createMoveQueue(sendInput: InputHandlerOptions['sendInput']) {
  let frame: number | null = null
  let queued: BrowserInputRequest | null = null
  return {
    cancel: () => {
      if (frame) window.cancelAnimationFrame(frame)
    },
    push: (input: BrowserInputRequest) => {
      queued = input
      if (frame) return
      frame = window.requestAnimationFrame(() => {
        frame = null
        if (queued) sendInput(queued)
        queued = null
      })
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
      // 保留浏览器原生 paste 事件，才能可靠拿到系统剪贴板内容。
      if (shortcut === 'paste') return
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

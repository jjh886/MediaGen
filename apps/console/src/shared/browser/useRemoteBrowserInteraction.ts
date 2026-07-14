import { useEffect, useMemo, useRef, useState } from 'react'
import type { BrowserPointerState } from '@/shared/services/browser-session-service'
import { createKeyboardHandlers, createPointerHandlers } from './remote-browser-input-handlers'
import type { ContextMenuPosition } from './RemoteBrowserContextMenu'
import { useRemoteBrowserActions } from './useRemoteBrowserActions'

const EMPTY_POINTER: BrowserPointerState = {
  cursor: 'default',
  editable: false,
  selectable: false,
  has_selection: false,
  password: false,
  element_kind: 'content',
}

interface InteractionOptions {
  sessionId: string
  viewport: { width: number; height: number }
  sendInput: Parameters<typeof createPointerHandlers>[0]['sendInput']
  onStatus: (status: string) => void
}

export function useRemoteBrowserInteraction(options: InteractionOptions) {
  const sinkRef = useRef<HTMLTextAreaElement | null>(null)
  const composingRef = useRef(false)
  const [pointer, setPointer] = useState(EMPTY_POINTER)
  const [menu, setMenu] = useState<ContextMenuPosition | null>(null)
  const actions = useRemoteBrowserActions({ ...options, sinkRef })
  const pointerInput = useMemo(
    () => createPointerHandlers({ ...options, focus: actions.focus, setMenu }),
    [options.sessionId, options.viewport.width, options.viewport.height, options.sendInput]
  )
  const keyHandlers = useMemo(
    () =>
      createKeyboardHandlers({
        ...options,
        composingRef,
        runAction: actions.runContextAction,
        sendText: actions.sendText,
      }),
    [options.sessionId, options.sendInput]
  )

  useEffect(() => () => pointerInput.cancel(), [pointerInput])
  const updatePointer = (next: BrowserPointerState) => next?.cursor && setPointer(next)

  return {
    sinkRef,
    pointer,
    updatePointer,
    menu,
    setMenu,
    pointerHandlers: pointerInput.handlers,
    keyHandlers,
    sendText: actions.sendText,
    copy: actions.copy,
    runContextAction: actions.runContextAction,
  }
}

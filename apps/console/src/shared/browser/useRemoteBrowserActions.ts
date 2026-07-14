import type { RefObject } from 'react'
import {
  browserSessionHistory,
  copyBrowserSelection,
  cutBrowserSelection,
  selectAllBrowserContent,
  writeBrowserClipboard,
} from '@/shared/services/browser-session-service'

interface BrowserActionOptions {
  sessionId: string
  sinkRef: RefObject<HTMLTextAreaElement | null>
  onStatus: (status: string) => void
}

export function useRemoteBrowserActions(options: BrowserActionOptions) {
  const focus = () => options.sinkRef.current?.focus({ preventScroll: true })
  const sendText = async (text: string) => {
    if (text) await writeBrowserClipboard(options.sessionId, text)
    focus()
  }
  const copy = async () => {
    const text = await copyBrowserSelection(options.sessionId)
    if (text) await navigator.clipboard.writeText(text).catch(() => undefined)
    options.onStatus(text ? '已复制远端选中内容' : '没有可复制的内容')
    focus()
  }
  const cut = async () => {
    const text = await cutBrowserSelection(options.sessionId)
    if (text) await navigator.clipboard.writeText(text).catch(() => undefined)
    focus()
  }
  const paste = async () => sendText(await navigator.clipboard.readText().catch(() => ''))
  const selectAll = async () => {
    await selectAllBrowserContent(options.sessionId)
    focus()
  }
  const history = async (action: 'back' | 'forward' | 'reload') => {
    await browserSessionHistory(options.sessionId, action)
    focus()
  }

  // 菜单和快捷键共用同一组动作，确保两种入口行为一致。
  const actionMap: Record<string, () => void> = {
    back: () => void history('back'),
    copy: () => void copy(),
    cut: () => void cut(),
    forward: () => void history('forward'),
    paste: () => void paste(),
    reload: () => void history('reload'),
    select_all: () => void selectAll(),
  }
  const runContextAction = (action: string) => actionMap[action]?.()

  return { copy, focus, runContextAction, sendText }
}

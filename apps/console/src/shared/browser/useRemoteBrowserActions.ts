import type { RefObject } from 'react'
import {
  browserSessionHistory,
  copyBrowserSelection,
  cutBrowserSelection,
  selectAllBrowserContent,
  type BrowserInputRequest,
} from '@/shared/services/browser-session-service'

interface BrowserActionOptions {
  sessionId: string
  sinkRef: RefObject<HTMLTextAreaElement | null>
  onStatus: (status: string) => void
  sendInput: (input: BrowserInputRequest) => void
}

export function useRemoteBrowserActions(options: BrowserActionOptions) {
  const focus = () => options.sinkRef.current?.focus({ preventScroll: true })
  const runAction = async (name: string, action: () => void | Promise<void>) => {
    try {
      await action()
    } catch {
      options.onStatus(`${name}未完成，请检查远程页面后重试`)
    } finally {
      focus()
    }
  }
  // 与键盘输入共用有序连接，避免粘贴另开 HTTP 请求经过加速节点。
  const sendText = (text: string) =>
    runAction('粘贴', () => {
      if (text) options.sendInput({ kind: 'ime', event: 'commit', text })
    })
  const copy = () =>
    runAction('复制', async () => {
      const text = await copyBrowserSelection(options.sessionId)
      if (text) await navigator.clipboard.writeText(text).catch(() => undefined)
      options.onStatus(text ? '已复制远端选中内容' : '没有可复制的内容')
    })
  const cut = () =>
    runAction('剪切', async () => {
      const text = await cutBrowserSelection(options.sessionId)
      if (text) await navigator.clipboard.writeText(text).catch(() => undefined)
    })
  const paste = () =>
    runAction('粘贴', async () => {
      await sendText(await navigator.clipboard.readText())
    })
  const selectAll = () =>
    runAction('全选', async () => {
      await selectAllBrowserContent(options.sessionId)
    })
  const history = (action: 'back' | 'forward' | 'reload') =>
    runAction('页面操作', async () => {
      await browserSessionHistory(options.sessionId, action)
    })

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

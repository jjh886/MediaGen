import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { runScriptJson, setCgptState } from './script-test-utils'

const originalFetch = window.fetch
const originalOpen = XMLHttpRequest.prototype.open
const originalSend = XMLHttpRequest.prototype.send

describe('chatgpt setup_interceptor', () => {
  beforeEach(() => {
    restoreBrowserHooks()
    setCgptState({})
  })

  afterEach(() => {
    restoreBrowserHooks()
  })

  it('不会把用户提示词中的错误关键词标记为平台失败', async () => {
    const state = await interceptMessage('user', '排除错误结构、无法生成的画面，遇到问题请重试。')

    expect(state.status).toBe('generating')
    expect(state.message).toBe('')
  })

  it('仍会识别助手回复的真实平台错误', async () => {
    const state = await interceptMessage('assistant', 'Something went wrong. Please try again.')

    expect(state.status).toBe('error')
    expect(state.message).toContain('Something went wrong')
  })
})

function restoreBrowserHooks() {
  window.fetch = originalFetch
  XMLHttpRequest.prototype.open = originalOpen
  XMLHttpRequest.prototype.send = originalSend
}

async function interceptMessage(role: string, messageText: string) {
  window.fetch = vi.fn(async () => {
    const message = {
      v: {
        conversation_id: 'conv-current',
        message: {
          author: { role },
          content: { parts: [messageText] },
        },
      },
    }
    return new Response(`data: ${JSON.stringify(message)}\n\n`, {
      headers: { 'Content-Type': 'text/event-stream' },
    })
  }) as typeof window.fetch
  setCgptState({ fetchHooked: false })

  await runScriptJson('setup_interceptor.js')
  await window.fetch('/backend-api/f/conversation')
  const holder = window as unknown as Record<symbol, Record<string, unknown>>
  const state = holder[Symbol.for('_cgpt')]
  // 流读取由拦截器异步执行，等到会话 ID 落盘后再断言错误状态。
  await vi.waitFor(() => expect(state.conversation_id).toBe('conv-current'))
  return state
}

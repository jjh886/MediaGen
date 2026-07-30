import { beforeEach, describe, expect, it, vi } from 'vitest'
import { runScriptJson, setCgptState } from './script-test-utils'

describe('chatgpt generate_image_poll', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    setCgptState({
      status: 'generating',
      startTime: Date.now(),
      fetchLog: [],
      uploaded_refs: [],
    })
    vi.useRealTimers()
  })

  it('ignores unrelated Chinese page text saying something went wrong', async () => {
    document.body.innerHTML = `
      <aside>历史记录：出了点问题</aside>
      <main>正在生成图片</main>
    `

    const result = await runScriptJson('generate_image_poll.js')

    expect(result.status).toBe('generating')
    expect(result.message).toBeUndefined()
  })
})

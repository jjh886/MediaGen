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

  it('ignores error phrases inside the user prompt', async () => {
    document.body.innerHTML = `
      <aside>历史记录：出了点问题</aside>
      <main>
        <section data-testid="conversation-turn-1">
          <div data-message-author-role="user">
            排除错误结构、无法生成、违反规则的画面，稍后重试也不要改变提示词。
          </div>
        </section>
        <section data-testid="conversation-turn-2">正在生成图片</section>
      </main>
    `

    const result = await runScriptJson('generate_image_poll.js')

    expect(result.status).toBe('generating')
    expect(result.message).toBeUndefined()
  })

  it('detects an actual error inside the assistant response', async () => {
    document.body.innerHTML = `
      <main>
        <section data-testid="conversation-turn-1">
          <div data-message-author-role="user">生成一张图片</div>
        </section>
        <section data-testid="conversation-turn-2">
          <div data-message-author-role="assistant">Something went wrong</div>
        </section>
      </main>
    `

    const result = await runScriptJson('generate_image_poll.js')

    expect(result.status).toBe('error')
    expect(result.error_code).toBe('platform_busy')
  })

  it('starts the ten-minute timeout from actual submission', async () => {
    const now = Date.now()
    setCgptState({
      status: 'generating',
      startTime: now - 20 * 60 * 1000,
      submit_started_at: now - (10 * 60 * 1000 - 1_000),
      fetchLog: [],
      uploaded_refs: [],
    })

    const result = await runScriptJson('generate_image_poll.js')

    expect(result.status).toBe('generating')
    expect(result.elapsed).toBeGreaterThanOrEqual(599)
  })

  it('reports timeout after ten minutes of actual generation', async () => {
    const now = Date.now()
    setCgptState({
      status: 'generating',
      startTime: now - 20 * 60 * 1000,
      submit_started_at: now - (10 * 60 * 1000 + 1_000),
      fetchLog: [],
      uploaded_refs: [],
    })

    const result = await runScriptJson('generate_image_poll.js')

    expect(result.status).toBe('error')
    expect(result.error_code).toBe('generation_timeout')
    expect(result.message).toBe('生成超时（10分钟）')
  })

  it('轮询会话时使用拦截器提供的鉴权请求方法', async () => {
    const authenticatedFetch = vi
      .fn()
      .mockResolvedValueOnce(Response.json({ status: 'IS_STREAMING' }))
      .mockResolvedValueOnce(Response.json({ mapping: {}, async_status: null }))
    setCgptState({
      status: 'generating',
      startTime: Date.now(),
      conversation_id: 'conv-authenticated',
      fetchLog: [],
      uploaded_refs: [],
      authenticatedFetch,
    })

    const result = await runScriptJson('generate_image_poll.js')

    expect(result.status).toBe('generating')
    expect(authenticatedFetch).toHaveBeenCalledWith(
      '/backend-api/conversation/conv-authenticated',
      { credentials: 'include' }
    )
  })
})

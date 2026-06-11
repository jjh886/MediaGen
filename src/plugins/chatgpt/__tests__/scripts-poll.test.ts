import { beforeEach, describe, expect, it, vi } from 'vitest'
import { runScriptJson, setCgptState } from '../../../../test-utils/chatgpt/script-test-utils'

describe('chatgpt polling scripts', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    vi.useRealTimers()
  })

  it('setup_interceptor keeps foreign conversation download pending instead of done', async () => {
    const originalFetch = window.fetch.bind(window)
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input)
        if (url.includes('/backend-api/files/download/')) {
          return new Response(
            JSON.stringify({
              download_url: 'https://chatgpt.com/backend-api/estuary/content?id=file_foreign',
            }),
            { headers: { 'Content-Type': 'application/json' } }
          )
        }
        return originalFetch(input)
      })
    )

    setCgptState({
      fetchHooked: false,
      status: 'generating',
      fetchLog: [],
      uploaded_refs: [],
      debugEvents: [],
    })

    await runScriptJson('setup_interceptor.js')
    await runScriptJson('mark_submit.js')
    await window.fetch(
      '/backend-api/files/download/file_foreign?conversation_id=conv-foreign&inline=false'
    )

    const stateHolder = window as unknown as Record<symbol, unknown>
    const state = stateHolder[Symbol.for('_cgpt')] as Record<string, unknown>
    expect(state.status).toBe('generating')
    expect(state.download_url).toBe('')
    expect((state.pending_result as { conversation_id?: string } | null)?.conversation_id).toBe(
      'conv-foreign'
    )
  })

  it('generate_image_poll ignores stale done result from previous task', async () => {
    const startTime = Date.now()
    setCgptState({
      status: 'done',
      startTime,
      result_at: startTime - 1_000,
      download_url: 'https://chatgpt.com/backend-api/estuary/content?id=file_old',
      generated_file_id: 'file_old',
      fetchLog: [],
      uploaded_refs: [],
    })

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('generating')
    expect(result.download_url).toBeUndefined()
  })

  it('generate_image_poll accepts a loaded rendered image when download interception misses', async () => {
    const startedAt = Date.now() - 2_000
    setCgptState({
      status: 'generating',
      startTime: Date.now() - 80_000,
      submit_started_at: startedAt,
      conversation_id: 'conv-current',
      fetchLog: [],
      uploaded_refs: ['file_ref_1'],
    })
    document.body.innerHTML = `
      <article data-message-author-role="assistant">
        <div id="image-result-1">
          <img alt="Generated image" src="blob:https://chatgpt.com/generated-result" />
        </div>
      </article>
    `
    const img = document.querySelector('img') as HTMLImageElement
    Object.defineProperty(img, 'naturalWidth', { configurable: true, value: 1024 })
    Object.defineProperty(img, 'naturalHeight', { configurable: true, value: 1024 })

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('done')
    expect(result.source).toBe('dom_rendered_image')
    expect(result.download_url).toBe('blob:https://chatgpt.com/generated-result')
  })

  it('generate_image_poll detects rate limit message from page text', async () => {
    document.body.innerHTML = `
      <main>
        <div>
          You're generating images too quickly. To ensure the best experience for everyone, we have rate limits in place.
          Please wait for 29 minutes before generating more images.
        </div>
      </main>
    `

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('error')
    expect(result.error_code).toBe('rate_limit_exceeded')
    expect(result.message).toContain('29 minutes')
  })

  it('generate_image_poll detects chinese rate limit modal and dismisses it', async () => {
    document.body.innerHTML = `
      <div role="dialog" aria-modal="true">
        <div>请求过于频繁</div>
        <div>你的请求过于频繁。为保障数据安全，我们已暂时限制你访问对话记录。请稍等几分钟后再重试。</div>
        <button id="confirm-btn">明白了</button>
      </div>
    `

    const confirmBtn = document.querySelector('#confirm-btn') as HTMLButtonElement
    const clickSpy = vi.spyOn(confirmBtn, 'click')

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('error')
    expect(result.error_code).toBe('rate_limit_exceeded')
    expect(result.message).toContain('请求过于频繁')
    expect(clickSpy).toHaveBeenCalledOnce()
  })

  it('generate_image_poll fails fast when ChatGPT returns a safety refusal text', async () => {
    document.body.innerHTML = `
      <main>
        <article>
          我不能帮你生成或描述涉及“杀人、抢劫等犯罪行为”的画面。
          如果你想做9:16的竖版镜头或电影级画面，我可以帮你改成安全版本。
        </article>
      </main>
    `

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('error')
    expect(result.error_code).toBe('content_policy_refusal')
    expect(result.message).toContain('ChatGPT 拒绝生成图片')
  })
})

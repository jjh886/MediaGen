import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it, beforeEach, vi } from 'vitest'

const here = path.dirname(fileURLToPath(import.meta.url))
const scriptsDir = path.resolve(here, '../../../../plugins/chatgpt/scripts')

function readScript(name: string) {
  return fs.readFileSync(path.join(scriptsDir, name), 'utf8')
}

function setCgptState(value: unknown) {
  const holder = window as unknown as { [key: symbol]: unknown }
  holder[Symbol.for('_cgpt')] = value
}

function markVisible(el: Element | null, rect = { width: 120, height: 32, x: 0, y: 0 }) {
  if (!el) return
  Object.defineProperty(el, 'getBoundingClientRect', {
    configurable: true,
    value: () => ({
      width: rect.width,
      height: rect.height,
      x: rect.x,
      y: rect.y,
      top: rect.y,
      left: rect.x,
      right: rect.x + rect.width,
      bottom: rect.y + rect.height,
      toJSON: () => ({}),
    }),
  })
}

async function runScriptJson(name: string) {
  const raw = window.eval(readScript(name))
  const value = raw instanceof Promise ? await raw : raw
  return JSON.parse(String(value))
}

describe('chatgpt plugin scripts', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    vi.useRealTimers()
  })

  it('select_ratio marks composer ratio trigger instead of history item options', async () => {
    document.body.innerHTML = `
      <aside>
        <button data-testid="history-item-0-options" aria-haspopup="menu" aria-label="打开“竖版镜头描述”的对话选项"></button>
      </aside>
      <main>
        <div id="composer-root">
          <button id="composer-plus-btn">+</button>
          <button>图片</button>
          <button id="model-trigger" aria-haspopup="menu" data-testid="model-switcher-dropdown-button">ChatGPT 模型选择器</button>
          <button id="ratio-trigger" aria-haspopup="menu">Auto Choose image aspect ratio</button>
          <div id="prompt-textarea" contenteditable="true"></div>
          <input id="upload-files" type="file" />
        </div>
      </main>
    `
    markVisible(document.querySelector('[data-testid="history-item-0-options"]'))
    markVisible(document.querySelector('#composer-plus-btn'))
    markVisible(document.querySelector('#model-trigger'))
    markVisible(document.querySelector('#ratio-trigger'))
    markVisible(document.querySelector('#prompt-textarea'))
    markVisible(document.querySelector('button:not(#composer-plus-btn):not(#ratio-trigger)'))

    setCgptState({ params: { ratio: '9:16' } })

    const result = await runScriptJson('select_ratio.js')
    expect(result.prepared).toBe(true)
    expect(result.trigger_text).toContain('auto choose image aspect ratio')
    expect(document.querySelector('[data-frame64-cgpt-ratio-trigger="true"]')).toBe(
      document.querySelector('#ratio-trigger')
    )
  })

  it('probe_image_mode prefers direct image entry inside current composer', async () => {
    document.body.innerHTML = `
      <aside>
        <button>生成图片</button>
      </aside>
      <main>
        <div id="composer-root">
          <button id="composer-plus-btn">+</button>
          <button id="image-entry">生成图片</button>
          <div id="prompt-textarea" contenteditable="true"></div>
          <input id="upload-files" type="file" />
        </div>
      </main>
    `
    markVisible(document.querySelector('aside button'))
    markVisible(document.querySelector('#composer-plus-btn'))
    markVisible(document.querySelector('#image-entry'))
    markVisible(document.querySelector('#prompt-textarea'))

    const result = await runScriptJson('probe_image_mode.js')
    expect(result.already_in_image_mode).toBe(false)
    expect(result.entry_via).toBe('direct')
    expect(result.skip_create_image_entry).toBe(true)
    expect(document.querySelector('[data-frame64-cgpt-image-entry="true"]')).toBe(
      document.querySelector('#image-entry')
    )
  })

  it('wait_create_image_entry finds create-image menu item', async () => {
    vi.useFakeTimers()
    document.body.innerHTML = `
      <div role="menu" id="menu">
        <div role="menuitemradio">创建图片</div>
      </div>
    `
    markVisible(document.querySelector('#menu'))
    markVisible(document.querySelector('[role="menuitemradio"]'))

    const pending = runScriptJson('wait_create_image_entry.js')
    await vi.advanceTimersByTimeAsync(150)
    const result = await pending
    expect(result.found).toBe(true)
    expect(result.target_text).toBe('创建图片')
  })

  it('wait_ratio_menu_option finds chinese ratio option by state params', async () => {
    vi.useFakeTimers()
    document.body.innerHTML = `
      <div role="menu" id="ratio-menu">
        <div class="__menu-label">Choose image aspect ratio</div>
        <div role="menuitemradio" aria-label="故事 9:16">故事 <span>9:16</span></div>
      </div>
    `
    markVisible(document.querySelector('#ratio-menu'))
    markVisible(document.querySelector('[role="menuitemradio"]'))

    setCgptState({ params: { ratio: '9:16', ratioLabel: '故事' } })

    const pending = runScriptJson('wait_ratio_menu_option.js')
    await vi.advanceTimersByTimeAsync(150)
    const result = await pending
    expect(result.found).toBe(true)
    expect(result.target_text).toBe('故事')
    expect(result.target_ratio).toBe('9:16')
    expect(result.selector).toBe('[data-frame64-cgpt-ratio-option="true"]')
    expect(document.querySelector('[data-frame64-cgpt-ratio-option="true"]')).toBe(
      document.querySelector('[role="menuitemradio"]')
    )
  })

  it('wait_ratio_menu_option marks ratio option even when menu label is missing', async () => {
    vi.useFakeTimers()
    document.body.innerHTML = `
      <div role="menu" id="ratio-menu">
        <div role="menuitemradio" aria-label="Square 1:1">Square <span>1:1</span></div>
        <div role="menuitemradio" aria-label="Story 9:16">Story <span>9:16</span></div>
        <div role="menuitemradio" aria-label="Landscape 16:9">Landscape <span>16:9</span></div>
      </div>
    `
    markVisible(document.querySelector('#ratio-menu'))
    document.querySelectorAll('[role="menuitemradio"]').forEach((el) => markVisible(el))

    setCgptState({ params: { ratio: '9:16', ratioLabel: '故事' } })

    const pending = runScriptJson('wait_ratio_menu_option.js')
    await vi.advanceTimersByTimeAsync(150)
    const result = await pending
    expect(result.found).toBe(true)
    expect(result.target_text).toBe('9:16')
    expect(result.option_text).toContain('Story 9:16')
  })

  it('setup_interceptor clears stale result when hooks already exist', async () => {
    setCgptState({
      fetchHooked: true,
      status: 'done',
      startTime: 1,
      download_url: 'https://chatgpt.com/backend-api/estuary/content?id=file_old',
      generated_file_id: 'file_old',
      conversation_id: 'old-conversation',
      image_gen_task_id: 'old-task',
      async_status: 'done',
      stream_status: 'COMPLETE',
      message: 'old',
      result_at: 2,
      fetchLog: [{ url: '/old' }],
      uploaded_refs: ['file_ref_old'],
      debugEvents: [{ kind: 'old' }],
      params: { ratio: '9:16' },
    })

    const beforeStart = Date.now()
    const result = await runScriptJson('setup_interceptor.js')
    const stateHolder = window as unknown as Record<symbol, unknown>
    const state = stateHolder[Symbol.for('_cgpt')] as Record<string, unknown>
    expect(result.hooked).toBe(true)
    expect(result.reused).toBe(true)
    expect(state.status).toBe('generating')
    expect(state.download_url).toBe('')
    expect(state.generated_file_id).toBe('')
    expect(state.conversation_id).toBe('')
    expect(state.image_gen_task_id).toBe('')
    expect(state.fetchLog).toEqual([])
    expect(state.uploaded_refs).toEqual([])
    expect(state.result_at).toBe(0)
    expect(state.pending_result).toBeNull()
    expect(state.submit_started_at).toBe(0)
    expect(state.startTime).toBeGreaterThanOrEqual(beforeStart)
    expect(typeof state.task_token).toBe('string')
    expect(state.task_token.length).toBeGreaterThan(8)
  })

  it('mark_submit resets result state right before enter', async () => {
    setCgptState({
      status: 'done',
      download_url: 'https://chatgpt.com/backend-api/estuary/content?id=file_old',
      generated_file_id: 'file_old',
      result_conversation_id: 'conv-old',
      conversation_id: 'conv-old',
      image_gen_task_id: 'task-old',
      async_status: 'done',
      stream_status: 'COMPLETE',
      message: 'old',
      result_at: 1,
      pending_result: { file_id: 'file_pending' },
    })

    const result = await runScriptJson('mark_submit.js')
    const stateHolder = window as unknown as Record<symbol, unknown>
    const state = stateHolder[Symbol.for('_cgpt')] as Record<string, unknown>
    expect(result.marked).toBe(true)
    expect(Number(state.submit_started_at)).toBeGreaterThan(0)
    expect(state.status).toBe('generating')
    expect(state.download_url).toBe('')
    expect(state.generated_file_id).toBe('')
    expect(state.result_conversation_id).toBe('')
    expect(state.conversation_id).toBe('')
    expect(state.pending_result).toBeNull()
  })

  it('wait_submit_ready waits for uploaded refs and marks enabled send button', async () => {
    vi.useFakeTimers()
    document.body.innerHTML = `
      <main>
        <form>
          <div id="prompt-textarea" contenteditable="true">prompt</div>
          <div aria-label="以全视图打开图片"></div>
          <button id="send" data-testid="send-button" aria-label="Send prompt" disabled></button>
        </form>
      </main>
    `
    markVisible(document.querySelector('#prompt-textarea'))
    markVisible(document.querySelector('[aria-label="以全视图打开图片"]'))
    markVisible(document.querySelector('#send'))
    setCgptState({ params: { refCount: 1 }, uploaded_refs: [] })

    const pending = runScriptJson('wait_submit_ready.js')
    await vi.advanceTimersByTimeAsync(600)
    const state = (window as unknown as Record<symbol, { uploaded_refs: string[] }>)[
      Symbol.for('_cgpt')
    ]
    state.uploaded_refs.push('file_ref')
    ;(document.querySelector('#send') as HTMLButtonElement).disabled = false
    await vi.advanceTimersByTimeAsync(600)

    const result = await pending
    expect(result.ready).toBe(true)
    expect(result.confirmedBy).toBe('upload_process_stream')
    expect(document.querySelector('[data-frame64-cgpt-send-button="true"]')).toBe(
      document.querySelector('#send')
    )
  })

  it('wait_attachments accepts english attachment preview card', async () => {
    vi.useFakeTimers()
    document.body.innerHTML = `
      <main>
        <div aria-label="Open image in full view"></div>
      </main>
    `
    markVisible(document.querySelector('[aria-label="Open image in full view"]'))
    setCgptState({ params: { refCount: 1 }, uploaded_refs: [] })

    const pending = runScriptJson('wait_attachments.js')
    await vi.advanceTimersByTimeAsync(600)
    const result = await pending
    expect(result.confirmedBy).toBe('dom_cards')
    expect(result.cards).toBe(1)
  })

  it('wait_submit_ready falls back to stable attachment cards before submit', async () => {
    vi.useFakeTimers()
    document.body.innerHTML = `
      <main>
        <form>
          <div id="prompt-textarea" contenteditable="true">prompt</div>
          <div aria-label="以全视图打开图片"></div>
          <button id="send" data-testid="send-button" aria-label="Send prompt"></button>
        </form>
      </main>
    `
    markVisible(document.querySelector('#prompt-textarea'))
    markVisible(document.querySelector('[aria-label="以全视图打开图片"]'))
    markVisible(document.querySelector('#send'))
    setCgptState({ params: { refCount: 1 }, uploaded_refs: [] })

    const pending = runScriptJson('wait_submit_ready.js')
    await vi.advanceTimersByTimeAsync(3600)

    const result = await pending
    expect(result.ready).toBe(true)
    expect(result.confirmedBy).toBe('dom_cards_stable')
  })

  it('wait_submit_ready reports rate limit when send button never becomes available', async () => {
    vi.useFakeTimers()
    document.body.innerHTML = `
      <main>
        <form>
          <div id="prompt-textarea" contenteditable="true">prompt</div>
        </form>
      </main>
    `
    markVisible(document.querySelector('#prompt-textarea'))
    setCgptState({ params: { refCount: 0 }, uploaded_refs: [] })

    const pending = runScriptJson('wait_submit_ready.js')
    await vi.advanceTimersByTimeAsync(61_000)

    const result = await pending
    expect(result.error_code).toBe('rate_limit_exceeded')
    expect(result.error).toContain('生成过于频繁')
    expect(result.error).toContain('请稍等几分钟')
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

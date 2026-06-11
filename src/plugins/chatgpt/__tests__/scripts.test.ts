import { describe, expect, it, beforeEach, vi } from 'vitest'
import {
  markVisible,
  runScriptJson,
  setCgptState,
} from '../../../../test-utils/chatgpt/script-test-utils'

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
    expect(document.querySelector('[data-mediagen-cgpt-ratio-trigger="true"]')).toBe(
      document.querySelector('#ratio-trigger')
    )
  })

  it('select_ratio keeps panorama 2:1 ratio instead of normalizing to story', async () => {
    document.body.innerHTML = `
      <main>
        <div id="composer-root">
          <button>图片</button>
          <button id="ratio-trigger" aria-haspopup="menu">Auto Choose image aspect ratio</button>
          <div id="prompt-textarea" contenteditable="true"></div>
          <input id="upload-files" type="file" />
        </div>
      </main>
    `
    markVisible(document.querySelector('button'))
    markVisible(document.querySelector('#ratio-trigger'))
    markVisible(document.querySelector('#prompt-textarea'))

    setCgptState({ params: { ratio: '2:1' } })

    const result = await runScriptJson('select_ratio.js')
    expect(result.prepared).toBe(true)
    expect(result.ratio).toBe('2:1')
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
    expect(document.querySelector('[data-mediagen-cgpt-image-entry="true"]')).toBe(
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

  it('wait_create_image_entry accepts english image item and ignores upload item', async () => {
    vi.useFakeTimers()
    document.body.innerHTML = `
      <div role="menu" id="menu">
        <div role="menuitem">Upload image</div>
        <div role="menuitem">Image</div>
      </div>
    `
    markVisible(document.querySelector('#menu'))
    document.querySelectorAll('[role="menuitem"]').forEach((el) => markVisible(el))

    const pending = runScriptJson('wait_create_image_entry.js')
    await vi.advanceTimersByTimeAsync(150)
    const result = await pending
    expect(result.found).toBe(true)
    expect(result.target_text).toBe('Image')
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
    expect(result.selector).toBe('[data-mediagen-cgpt-ratio-option="true"]')
    expect(document.querySelector('[data-mediagen-cgpt-ratio-option="true"]')).toBe(
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
    expect(result.option_text).toContain('9:16')
    expect(result.option_text).toContain('Story 9:16')
  })

  it('wait_ratio_menu_option falls back to auto for panorama ratio when 2:1 is unavailable', async () => {
    vi.useFakeTimers()
    document.body.innerHTML = `
      <div role="menu" id="ratio-menu">
        <div class="__menu-label">Choose image aspect ratio</div>
        <div role="menuitemradio" aria-label="Auto">Auto</div>
        <div role="menuitemradio" aria-label="Story 9:16">Story <span>9:16</span></div>
        <div role="menuitemradio" aria-label="Landscape 16:9">Landscape <span>16:9</span></div>
      </div>
    `
    markVisible(document.querySelector('#ratio-menu'))
    document.querySelectorAll('[role="menuitemradio"]').forEach((el) => markVisible(el))

    setCgptState({ params: { ratio: '2:1', ratioLabel: '2:1' } })

    const pending = runScriptJson('wait_ratio_menu_option.js')
    await vi.advanceTimersByTimeAsync(150)
    const result = await pending
    expect(result.found).toBe(true)
    expect(result.target_text).toBe('Auto')
    expect(result.target_ratio).toBe('2:1')
    expect(result.effective_ratio).toBe('2:1')
    expect(result.fallback).toBe('auto_for_2_1')
  })

  it('wait_ratio_menu_option falls back to story when auto option is unavailable', async () => {
    vi.useFakeTimers()
    document.body.innerHTML = `
      <div role="menu" id="ratio-menu">
        <div class="__menu-label">Choose image aspect ratio</div>
        <div role="menuitemradio" aria-label="Story 9:16">Story <span>9:16</span></div>
        <div role="menuitemradio" aria-label="Landscape 16:9">Landscape <span>16:9</span></div>
        <div role="menuitemradio" aria-label="Square 1:1">Square <span>1:1</span></div>
      </div>
    `
    markVisible(document.querySelector('#ratio-menu'))
    document.querySelectorAll('[role="menuitemradio"]').forEach((el) => markVisible(el))

    setCgptState({ params: { ratio: 'auto', ratioLabel: 'Auto' } })

    const pending = runScriptJson('wait_ratio_menu_option.js')
    await vi.advanceTimersByTimeAsync(150)
    const result = await pending
    expect(result.found).toBe(true)
    expect(result.option_text).toContain('9:16')
    expect(result.effective_ratio).toBe('9:16')
    expect(result.fallback).toBe('story_for_auto')
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

  it('setup_interceptor remembers ChatGPT api headers for cleanup calls', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}'))
    )
    setCgptState({ fetchHooked: false, fetchLog: [], uploaded_refs: [], debugEvents: [] })

    await runScriptJson('setup_interceptor.js')
    await window.fetch('/backend-api/conversations', {
      headers: {
        'oai-device-id': 'device-1',
        'oai-session-id': 'session-1',
      },
    })

    const stateHolder = window as unknown as Record<symbol, Record<string, unknown>>
    const state = stateHolder[Symbol.for('_cgpt')]
    expect(state.api_headers).toEqual(
      expect.objectContaining({
        'oai-device-id': 'device-1',
        'oai-session-id': 'session-1',
      })
    )
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
    expect(document.querySelector('[data-mediagen-cgpt-send-button="true"]')).toBe(
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
})

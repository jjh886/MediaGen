import { beforeEach, describe, expect, it, vi } from 'vitest'
import { markVisible, runScriptJson } from '../../../../test-utils/chatgpt/script-test-utils'

describe('chatgpt prepare_page', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    vi.useRealTimers()
  })

  it('succeeds when composer is ready even if upload input is delayed', async () => {
    document.body.innerHTML = `
      <main>
        <form>
          <button id="composer-plus-btn">+</button>
          <div id="prompt-textarea" contenteditable="true"></div>
        </form>
      </main>
    `
    markVisible(document.querySelector('#composer-plus-btn'))
    markVisible(document.querySelector('#prompt-textarea'))

    const result = await runScriptJson('prepare_page.js')

    expect(result.prepared).toBe(true)
    expect(result.anyComposer).toBe(true)
    expect(result.upload_ready).toBe(false)
    expect(result.plusButtons).toBe(1)
  })

  it('falls back to generic textbox composer selector', async () => {
    document.body.innerHTML = `
      <main>
        <form>
          <div role="textbox" contenteditable="true">draft</div>
        </form>
      </main>
    `
    markVisible(document.querySelector('[role="textbox"]'))

    const result = await runScriptJson('prepare_page.js')

    expect(result.prepared).toBe(true)
    expect(result.anyComposer).toBe(true)
  })
})

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { markVisible, runScriptJson } from './script-test-utils'

describe('chatgpt prepare_page', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    vi.useRealTimers()
  })

  it('marks the standard composer even when upload input is delayed', async () => {
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
    expect(result.upload_ready).toBe(false)
    expect(result.composer_selector).toBe('[data-mediagen-cgpt-composer="true"]')
    expect(document.querySelector(result.composer_selector)).toBe(
      document.querySelector('#prompt-textarea')
    )
  })

  it('marks a generic textbox when ChatGPT omits prompt-textarea', async () => {
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
    expect(result.promptCount).toBe(0)
    expect(document.querySelector(result.composer_selector)).toBe(
      document.querySelector('[role="textbox"]')
    )
  })
})

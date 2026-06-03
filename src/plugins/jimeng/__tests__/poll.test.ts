import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  resetJimengDom,
  runScriptJson,
  setJimengState,
} from '../../../../plugins/jimeng/__tests__/test-utils'

describe('jimeng plugin scripts', () => {
  beforeEach(() => {
    resetJimengDom()
  })

  it('generate_image_poll detects and dismisses jimeng chinese rate limit modal', async () => {
    document.body.innerHTML = `
      <div role="dialog">
        <div>请求过于频繁</div>
        <div>你的请求过于频繁。请稍等几分钟后再重试。</div>
        <button id="ok">明白了</button>
      </div>
    `
    const button = document.querySelector('#ok') as HTMLButtonElement
    const clickSpy = vi.spyOn(button, 'click')

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('error')
    expect(result.error_code).toBe('rate_limit_exceeded')
    expect(result.message).toContain('请求过于频繁')
    expect(clickSpy).toHaveBeenCalledOnce()
  })

  it('generate_image_poll detects and closes jimeng quota modal', async () => {
    document.body.innerHTML = `
      <div role="dialog">
        <div>积分不足或没有相关权益</div>
        <div>你没有足够的积分进行此操作，请升级会员获得更多积分</div>
        <button id="close" aria-label="关闭">×</button>
        <button id="upgrade">升级会员得积分</button>
      </div>
    `
    const closeButton = document.querySelector('#close') as HTMLButtonElement
    const clickSpy = vi.spyOn(closeButton, 'click')

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('error')
    expect(result.error_code).toBe('quota_or_entitlement_insufficient')
    expect(result.retryable).toBe(false)
    expect(result.message).toContain('积分不足')
    expect(clickSpy).toHaveBeenCalledOnce()
  })

  it('generate_image_poll does not finish from unbound DOM images', async () => {
    document.body.innerHTML =
      '<img id="result" src="https://p26-dreamina-sign.byteimg.com/foo~tplv-tb4s082cfz-aigc_resize_mark:0:0.png">'
    const img = document.querySelector('#result') as HTMLImageElement
    Object.defineProperty(img, 'naturalWidth', { configurable: true, value: 1440 })
    Object.defineProperty(img, 'naturalHeight', { configurable: true, value: 2560 })
    setJimengState({
      status: 'generating',
      startTime: Date.now(),
      submit_started_at: Date.now() - 1000,
      submit_id: 'submit_1',
      initialImageUrls: [],
      uploaded_refs: [],
      fetchLog: [],
    })

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('generating')
    expect(result.source).toBeUndefined()
  })

  it('generate_image_poll errors when submit click navigates to work detail', async () => {
    window.history.pushState({}, '', '/ai-tool/work-detail/7632184435516935433?type=image')
    setJimengState({
      status: 'generating',
      startTime: Date.now(),
      submit_started_at: Date.now() - 1000,
      submit_id: '',
      uploaded_refs: [],
      fetchLog: [],
    })

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('error')
    expect(result.error_code).toBe('jimeng_wrong_page_after_submit')
    expect(result.retryable).toBe(true)
  })

  it('generate_image_poll keeps waiting when submit id is delayed', async () => {
    window.history.pushState({}, '', '/ai-tool/home?type=image')
    const now = Date.now()
    setJimengState({
      status: 'generating',
      startTime: now - 100000,
      submit_started_at: now - 95000,
      submit_id: '',
      uploaded_refs: [],
      fetchLog: [{ url: '/mweb/v1/other', at: now - 90000 }],
    })

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('generating')
    expect(result.submit_pending_long).toBe(true)
    expect(result.diagnostic).toContain('继续等待平台自恢复')
  })

  it('generate_image_poll binds current active conversation after submit_id is known', async () => {
    document.body.innerHTML = `
      <div class="tooltip-trigger-shell-bkggBT" id="target">
        <div class="conversation-item-ntazVf active-XWsVs1">
          <span class="item-name-lLvLeT">未命名对话</span>
          <div class="more-button-z412Bp"></div>
        </div>
      </div>
    `
    setJimengState({
      status: 'generating',
      startTime: Date.now(),
      submit_started_at: Date.now() - 1000,
      submit_id: 'submit_1',
      task_token: 'jimeng_task_1',
      uploaded_refs: [],
      fetchLog: [],
    })

    const result = await runScriptJson('generate_image_poll.js')
    expect(result.status).toBe('generating')
    expect(result.conversation.marked).toBe(true)
    expect(result.conversation.submit_id).toBe('submit_1')
    expect(
      document.querySelector('#target')?.getAttribute('data-mediagen-jimeng-task-conversation')
    ).toBe('jimeng_task_1')
  })
})

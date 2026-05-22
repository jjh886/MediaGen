import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  getJimengState,
  markVisible,
  resetJimengDom,
  runScriptJson,
  setJimengState,
} from '../../../../plugins/jimeng/__tests__/test-utils'

describe('jimeng plugin scripts', () => {
  beforeEach(() => {
    resetJimengDom()
  })

  it('setup_interceptor extracts all generated images from get_history_by_ids', async () => {
    const originalFetch = window.fetch.bind(window)
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({
              ret: '0',
              data: {
                submit_1: {
                  item_list: [
                    {
                      image: {
                        large_images: [
                          {
                            image_url: 'https://p26-dreamina-sign.byteimg.com/result-a.png',
                            image_uri: 'tos-cn-i-tb4s082cfz/result-a',
                            width: 1440,
                            height: 2560,
                          },
                        ],
                      },
                    },
                    {
                      image: {
                        large_images: [
                          {
                            image_url: 'https://p26-dreamina-sign.byteimg.com/result-b.png',
                            image_uri: 'tos-cn-i-tb4s082cfz/result-b',
                            width: 1440,
                            height: 2560,
                          },
                        ],
                      },
                    },
                  ],
                  task: { status: 45 },
                },
              },
            }),
            { headers: { 'Content-Type': 'application/json' } }
          )
      )
    )

    setJimengState({ params: { maxImages: 1 } })
    await runScriptJson('setup_interceptor.js')
    await runScriptJson('mark_submit.js')
    getJimengState().submit_id = 'submit_1'

    await window.fetch('/mweb/v1/get_history_by_ids', {
      method: 'POST',
      body: JSON.stringify({ submit_ids: ['submit_1'] }),
    })
    await new Promise((resolve) => setTimeout(resolve, 0))

    const state = getJimengState()
    expect(state.status).toBe('done')
    expect(state.imageCount).toBe(2)
    expect(state.images).toEqual([
      {
        url: 'https://p26-dreamina-sign.byteimg.com/result-a.png',
        image_uri: 'tos-cn-i-tb4s082cfz/result-a',
        width: 1440,
        height: 2560,
      },
      {
        url: 'https://p26-dreamina-sign.byteimg.com/result-b.png',
        image_uri: 'tos-cn-i-tb4s082cfz/result-b',
        width: 1440,
        height: 2560,
      },
    ])

    vi.stubGlobal('fetch', originalFetch)
  })

  it('setup_interceptor ignores stale history responses from an old task token', async () => {
    const originalFetch = window.fetch.bind(window)
    let resolveFetch: (() => void) | undefined
    vi.stubGlobal(
      'fetch',
      vi.fn(
        () =>
          new Promise<Response>((resolve) => {
            resolveFetch = () =>
              resolve(
                new Response(
                  JSON.stringify({
                    ret: '0',
                    data: {
                      old_submit: {
                        item_list: [
                          {
                            image: {
                              large_images: [
                                {
                                  image_url: 'https://p26-dreamina-sign.byteimg.com/old-result.png',
                                  image_uri: 'tos-cn-i-tb4s082cfz/old-result',
                                  width: 1440,
                                  height: 2560,
                                },
                              ],
                            },
                          },
                        ],
                        task: { status: 45 },
                      },
                    },
                  }),
                  { headers: { 'Content-Type': 'application/json' } }
                )
              )
          })
      )
    )

    setJimengState({ params: {} })
    await runScriptJson('setup_interceptor.js')
    await runScriptJson('mark_submit.js')
    getJimengState().submit_id = 'old_submit'

    const pending = window.fetch('/mweb/v1/get_history_by_ids', {
      method: 'POST',
      body: JSON.stringify({ submit_ids: ['old_submit'] }),
    })

    await runScriptJson('mark_submit.js')
    getJimengState().submit_id = 'new_submit'
    resolveFetch?.()
    await pending
    await new Promise((resolve) => setTimeout(resolve, 0))

    const state = getJimengState()
    expect(state.status).toBe('generating')
    expect(state.images).toEqual([])

    vi.stubGlobal('fetch', originalFetch)
  })

  it('setup_interceptor patches jimeng generate payload model and ratio before request', async () => {
    const originalFetch = window.fetch.bind(window)
    let capturedBody = ''
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_input, init) => {
        capturedBody = String(init?.body || '')
        return new Response(
          JSON.stringify({
            ret: '0',
            data: {
              aigc_data: {
                task: { submit_id: 'submit_1' },
                history_record_id: 'history_1',
              },
            },
          }),
          { headers: { 'Content-Type': 'application/json' } }
        )
      })
    )

    setJimengState({
      params: {
        modelDisplayName: 'jimeng_图片_4.1',
        ratio: '9:16',
        prompt: '测试提示词',
      },
    })
    await runScriptJson('setup_interceptor.js')

    const draft = {
      component_list: [
        {
          abilities: {
            blend: {
              core_param: {
                model: 'high_aes_general_v50',
                prompt: 'old',
                image_ratio: 1,
                large_image_info: { width: 2048, height: 2048, resolution_type: '2k' },
                intelligent_ratio: true,
              },
            },
          },
        },
      ],
    }
    await window.fetch('/mweb/v1/aigc_draft/generate', {
      method: 'POST',
      body: JSON.stringify({
        extend: { root_model: 'high_aes_general_v50' },
        submit_id: 'submit_1',
        metrics_extra: JSON.stringify({
          sceneOptions: JSON.stringify([
            { modelReqKey: 'high_aes_general_v50', resolutionType: '2k' },
          ]),
        }),
        draft_content: JSON.stringify(draft),
      }),
    })

    const patched = JSON.parse(capturedBody)
    const patchedDraft = JSON.parse(patched.draft_content)
    const core = patchedDraft.component_list[0].abilities.blend.core_param
    expect(patched.extend.root_model).toBe('high_aes_general_v41')
    expect(core.model).toBe('high_aes_general_v41')
    expect(core.prompt).toContain('测试提示词')
    expect(core.image_ratio).toBe(5)
    expect(core.large_image_info).toMatchObject({
      width: 1440,
      height: 2560,
      resolution_type: '2k',
    })
    expect(core.intelligent_ratio).toBe(false)

    vi.stubGlobal('fetch', originalFetch)
  })

  it('select_model marks trigger and wait_model_option marks target model option', async () => {
    document.body.innerHTML = `
      <div role="combobox" id="kind">图片生成</div>
      <div role="combobox" id="model">图片5.0 Lite</div>
      <div role="listbox">
        <li role="option" id="lite">图片5.0 Lite</li>
        <li role="option" id="target">
          <div class="option-label-Fv9c0E">图片 4.1</div>
          <span>更专业的创意、美学和一致性保持</span>
        </li>
      </div>
    `
    const model = document.querySelector('#model')
    const target = document.querySelector('#target')
    markVisible(document.querySelector('#kind'))
    markVisible(model)
    markVisible(document.querySelector('#lite'))
    markVisible(target)
    setJimengState({ params: { modelDisplayName: 'jimeng_图片_4.1' } })

    const prepared = await runScriptJson('select_model.js')
    expect(prepared.prepared).toBe(true)
    expect(prepared.target).toBe('图片 4.1')
    expect(model?.getAttribute('data-frame64-jimeng-model-trigger')).toBe('true')

    const option = await runScriptJson('wait_model_option.js')
    expect(option.found).toBe(true)
    expect(option.selector).toBe('[data-frame64-jimeng-model-option="true"]')
    expect(target?.getAttribute('data-frame64-jimeng-model-option')).toBe('true')
  })

  it('select_ratio marks trigger and wait_ratio_option marks target ratio option', async () => {
    document.body.innerHTML = `
      <button id="ratio" class="toolbar-button-mCaZcW lv-btn-secondary">
        <span class="button-text-ZgrDpX">智能比例<div>高清 2K</div></span>
      </button>
      <div class="lv-popover-content">
        <label class="lv-radio"><input type="radio" value="1:1"><span>1:1</span></label>
        <label class="lv-radio"><input type="radio" value="9:16"><span>9:16</span></label>
      </div>
    `
    const ratioButton = document.querySelector('#ratio')
    const target = document.querySelector('input[value="9:16"]')?.closest('label') || null
    markVisible(ratioButton)
    document.querySelectorAll('label').forEach(markVisible)
    setJimengState({ params: { ratio: '9:16' } })

    const prepared = await runScriptJson('select_ratio.js')
    expect(prepared.prepared).toBe(true)
    expect(prepared.trigger_selector).toBe('[data-frame64-jimeng-ratio-trigger="true"]')
    expect(ratioButton?.getAttribute('data-frame64-jimeng-ratio-trigger')).toBe('true')

    const option = await runScriptJson('wait_ratio_option.js')
    expect(option.found).toBe(true)
    expect(option.selector).toBe('[data-frame64-jimeng-ratio-option="true"]')
    expect(target?.getAttribute('data-frame64-jimeng-ratio-option')).toBe('true')
  })

  it('wait_submit_ready marks a usable jimeng submit button', async () => {
    document.body.innerHTML = `
      <div class="work-detail">
        <button class="lv-btn lv-btn-primary submit-button-xdhu0e" id="wrong" type="button">做同款</button>
      </div>
      <div class="layout-iv0WDF">
        <div class="references-F9DFuS">
          <input type="file" accept="image/png">
        </div>
        <div class="prompt-editor">
          <div class="ProseMirror" contenteditable="true" role="textbox">prompt 画面比例：9:16</div>
        </div>
        <div class="toolbar-actions">
          <button class="lv-btn lv-btn-primary submit-button-xdhu0e" id="submit" type="button"></button>
        </div>
      </div>
    `
    markVisible(document.querySelector('.ProseMirror'))
    markVisible(document.querySelector('#wrong'))
    markVisible(document.querySelector('#submit'))
    setJimengState({
      params: {
        prompt: 'prompt',
        ratio: '9:16',
        modelDisplayName: 'jimeng_图片_4.1',
        refCount: 0,
      },
      last_prompt_text: 'prompt\n画面比例：9:16',
      model_ready: true,
      ratio_ready: true,
      fetchHooked: true,
      uploaded_refs: [],
    })

    const result = await runScriptJson('wait_submit_ready.js')
    expect(result.ready).toBe(true)
    expect(result.selector).toBe('[data-frame64-jimeng-submit="true"]')
    expect(document.querySelector('[data-frame64-jimeng-submit="true"]')).toBe(
      document.querySelector('#submit')
    )
  })

  it('wait_submit_ready repairs a missing prompt before enabling submit', async () => {
    document.body.innerHTML = `
      <div class="layout-iv0WDF">
        <div class="references-F9DFuS">
          <input type="file" accept="image/png">
        </div>
        <div class="prompt-editor">
          <div class="ProseMirror" contenteditable="true" role="textbox">画面比例：9:16</div>
        </div>
        <div class="toolbar-actions">
          <button class="lv-btn lv-btn-primary submit-button-xdhu0e" id="submit" type="button"></button>
        </div>
      </div>
    `
    const editor = document.querySelector('.ProseMirror')
    markVisible(editor)
    markVisible(document.querySelector('#submit'))
    setJimengState({
      params: {
        prompt: 'prompt',
        ratio: '9:16',
        modelDisplayName: 'jimeng_图片_4.1',
        refCount: 0,
      },
      last_prompt_text: 'prompt\n画面比例：9:16',
      model_ready: true,
      ratio_ready: true,
      fetchHooked: true,
      uploaded_refs: [],
    })

    const result = await runScriptJson('wait_submit_ready.js')
    expect(result.ready).toBe(true)
    expect(editor?.textContent).toContain('prompt')
    expect(editor?.textContent).toContain('画面比例：9:16')
    expect(document.querySelector('[data-frame64-jimeng-submit="true"]')).toBe(
      document.querySelector('#submit')
    )
  })

  it('confirm_submit_started retries the marked submit button until generate starts', async () => {
    document.body.innerHTML = `
      <button
        class="lv-btn lv-btn-primary lv-btn-size-default lv-btn-shape-circle lv-btn-icon-only submit-button-xdhu0e"
        data-frame64-jimeng-submit="true"
        type="button"
        id="submit"
      ></button>
    `
    const button = document.querySelector('#submit') as HTMLButtonElement
    markVisible(button)
    let clicks = 0
    setJimengState({
      submit_started_at: Date.now(),
      submit_id: '',
      generate_request_count: 0,
      generate_request_started_at: 0,
      fetchLog: [],
      submit_confirm_initial_wait_ms: 1,
      submit_confirm_retry_wait_ms: 1,
      submit_confirm_attempts: 2,
    })
    button.addEventListener('click', () => {
      clicks += 1
      const state = getJimengState()
      state.generate_request_count = 1
      state.generate_request_started_at = Date.now()
    })

    const result = await runScriptJson('confirm_submit_started.js')
    expect(result.confirmed).toBe(true)
    expect(result.reason).toBe('retry_click_started_submit')
    expect(clicks).toBeGreaterThan(0)
    expect(getJimengState().submit_confirmed).toBe(true)
  })

  it('confirm_submit_started reports missing submit button without failing poll flow', async () => {
    setJimengState({
      submit_started_at: Date.now(),
      submit_id: '',
      generate_request_count: 0,
      generate_request_started_at: 0,
      fetchLog: [],
      submit_confirm_initial_wait_ms: 1,
      submit_confirm_retry_wait_ms: 1,
      submit_confirm_attempts: 1,
    })

    const result = await runScriptJson('confirm_submit_started.js')
    expect(result.confirmed).toBe(false)
    expect(result.reason).toBe('submit_button_missing')
    expect(result.error).toBeUndefined()
  })
})

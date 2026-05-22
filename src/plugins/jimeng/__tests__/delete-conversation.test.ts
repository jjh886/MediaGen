import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  markVisible,
  resetJimengDom,
  runScriptJson,
  setJimengState,
} from '../../../../plugins/jimeng/__tests__/test-utils'

describe('jimeng plugin scripts', () => {
  beforeEach(() => {
    resetJimengDom()
  })

  it('delete_current_conversation deletes only the conversation marked by mark_submit', async () => {
    document.body.innerHTML = `
      <div class="list-LIKXl7">
        <div class="tooltip-trigger-shell-bkggBT" id="keep">
          <div class="conversation-item-ntazVf">
            <span class="item-name-lLvLeT">未命名对话</span>
            <div class="more-button-z412Bp"></div>
          </div>
        </div>
        <div class="tooltip-trigger-shell-bkggBT" id="target">
          <div class="conversation-item-ntazVf active-XWsVs1">
            <img src="https://p26-dreamina-sign.byteimg.com/tos-cn-i-tb4s082cfz/aaaaaaaaaaaaaaaaaaaaaaaa~tplv-tb4s082cfz-resize:100:100.image" alt="未命名对话">
            <span class="item-name-lLvLeT">未命名对话</span>
            <div class="more-button-z412Bp"></div>
          </div>
        </div>
      </div>
      <div class="lv-popover-content" role="tooltip">
        <div class="menu-item-F7C26Q" id="delete-menu"><span>删除</span></div>
      </div>
    `
    const target = document.querySelector('#target')
    const deleteMenu = document.querySelector('#delete-menu') as HTMLElement
    markVisible(document.querySelector('.lv-popover-content'))
    markVisible(deleteMenu)
    deleteMenu.addEventListener('click', () => target?.remove())

    setJimengState({})
    const marked = await runScriptJson('mark_submit.js')
    expect(marked.conversation.marked).toBe(true)
    expect(target?.hasAttribute('data-frame64-jimeng-task-conversation')).toBe(true)

    const result = await runScriptJson('delete_current_conversation.js')
    expect(result.deleted).toBe(true)
    expect(document.querySelector('#target')).toBeNull()
    expect(document.querySelector('#keep')).not.toBeNull()
  })

  it('delete_current_conversation skips when the task conversation was not marked', async () => {
    document.body.innerHTML = `
      <div class="tooltip-trigger-shell-bkggBT" id="target">
        <div class="conversation-item-ntazVf active-XWsVs1">
          <span class="item-name-lLvLeT">未命名对话</span>
          <div class="more-button-z412Bp"></div>
        </div>
      </div>
      <div class="lv-popover-content" role="tooltip">
        <div class="menu-item-F7C26Q" id="delete-menu"><span>删除</span></div>
      </div>
    `
    const deleteMenu = document.querySelector('#delete-menu') as HTMLElement
    const clickSpy = vi.spyOn(deleteMenu, 'click')
    markVisible(document.querySelector('.lv-popover-content'))
    markVisible(deleteMenu)
    setJimengState({ task_token: 'jimeng_old' })

    const result = await runScriptJson('delete_current_conversation.js')
    expect(result.skipped).toBe(true)
    expect(result.deleted).toBe(false)
    expect(clickSpy).not.toHaveBeenCalled()
    expect(document.querySelector('#target')).not.toBeNull()
  })

  it('delete_current_conversation deletes active conversation when thumbnail matches generated result', async () => {
    const resultKey = '6f973c293f8844af9ea05c16c9b6ae84'
    document.body.innerHTML = `
      <div class="tooltip-trigger-shell-bkggBT" id="target">
        <div class="conversation-item-ntazVf active-XWsVs1">
          <img src="https://p11-dreamina-sign.byteimg.com/tos-cn-i-tb4s082cfz/${resultKey}~tplv-tb4s082cfz-resize:100:100.image" alt="未命名对话">
          <span class="item-name-lLvLeT">未命名对话</span>
          <div class="more-button-z412Bp"></div>
        </div>
      </div>
      <div class="lv-popover-content" role="tooltip">
        <div class="menu-item-F7C26Q" id="delete-menu"><span>删除</span></div>
      </div>
    `
    const target = document.querySelector('#target')
    const deleteMenu = document.querySelector('#delete-menu') as HTMLElement
    markVisible(document.querySelector('.lv-popover-content'))
    markVisible(deleteMenu)
    deleteMenu.addEventListener('click', () => target?.remove())

    setJimengState({
      images: [
        {
          image_uri: `tos-cn-i-tb4s082cfz/${resultKey}`,
          url: `https://p11-dreamina-sign.byteimg.com/tos-cn-i-tb4s082cfz/${resultKey}~tplv-tb4s082cfz-aigc_resize:0:0.png`,
        },
      ],
      conversation_snapshot: { marked: false, reason: '未找到 active 会话' },
    })

    const result = await runScriptJson('delete_current_conversation.js')
    expect(result.deleted).toBe(true)
    expect(result.source).toBe('active_result_thumbnail')
    expect(document.querySelector('#target')).toBeNull()
  })

  it('delete_current_conversation waits for active thumbnail before deleting', async () => {
    const resultKey = 'c1ce3b289f06487c975faef362ead1e1'
    document.body.innerHTML = `
      <div class="tooltip-trigger-shell-bkggBT" id="target">
        <div class="conversation-item-ntazVf active-XWsVs1">
          <div class="item-media-img-yqwQIp"><img id="thumb" alt="未命名对话"></div>
          <span class="item-name-lLvLeT">未命名对话</span>
          <div class="more-button-z412Bp"></div>
        </div>
      </div>
      <div class="lv-popover-content" role="tooltip">
        <div class="menu-item-F7C26Q" id="delete-menu"><span>删除</span></div>
      </div>
    `
    const target = document.querySelector('#target')
    const deleteMenu = document.querySelector('#delete-menu') as HTMLElement
    const thumb = document.querySelector('#thumb') as HTMLImageElement
    markVisible(document.querySelector('.lv-popover-content'))
    markVisible(deleteMenu)
    deleteMenu.addEventListener('click', () => target?.remove())
    window.setTimeout(() => {
      thumb.src = `https://p11-dreamina-sign.byteimg.com/tos-cn-i-tb4s082cfz/${resultKey}~tplv-tb4s082cfz-resize:100:100.image`
    }, 50)

    setJimengState({
      images: [{ image_uri: `tos-cn-i-tb4s082cfz/${resultKey}` }],
      conversation_snapshot: { marked: false, reason: '未找到 active 会话' },
    })

    const result = await runScriptJson('delete_current_conversation.js')
    expect(result.deleted).toBe(true)
    expect(result.assetKey).toBe(resultKey)
    expect(document.querySelector('#target')).toBeNull()
  })

  it('delete_current_conversation skips active conversation when thumbnail does not match generated result', async () => {
    const activeKey = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    const resultKey = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
    document.body.innerHTML = `
      <div class="tooltip-trigger-shell-bkggBT" id="target">
        <div class="conversation-item-ntazVf active-XWsVs1">
          <img src="https://p11-dreamina-sign.byteimg.com/tos-cn-i-tb4s082cfz/${activeKey}~tplv-tb4s082cfz-resize:100:100.image" alt="未命名对话">
          <span class="item-name-lLvLeT">未命名对话</span>
          <div class="more-button-z412Bp"></div>
        </div>
      </div>
      <div class="lv-popover-content" role="tooltip">
        <div class="menu-item-F7C26Q" id="delete-menu"><span>删除</span></div>
      </div>
    `
    const deleteMenu = document.querySelector('#delete-menu') as HTMLElement
    const clickSpy = vi.spyOn(deleteMenu, 'click')
    markVisible(document.querySelector('.lv-popover-content'))
    markVisible(deleteMenu)

    setJimengState({
      images: [{ image_uri: `tos-cn-i-tb4s082cfz/${resultKey}` }],
      conversation_snapshot: { marked: false, reason: '未找到 active 会话' },
    })

    const result = await runScriptJson('delete_current_conversation.js')
    expect(result.skipped).toBe(true)
    expect(result.deleted).toBe(false)
    expect(result.currentAsset).toBe(activeKey)
    expect(clickSpy).not.toHaveBeenCalled()
    expect(document.querySelector('#target')).not.toBeNull()
  })
})

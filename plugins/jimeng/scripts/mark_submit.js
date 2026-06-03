;(() => {
  const SK = Symbol.for('_jimeng')
  const state = (window[SK] = window[SK] || {})
  const nextTaskToken = () => `jimeng_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
  const conversationMarkerAttr = 'data-mediagen-jimeng-task-conversation'

  const textOf = (el) => (el?.textContent || '').replace(/\s+/g, ' ').trim()
  const classOf = (el) => String(el?.className || '')
  const isConversationItem = (el) => /conversation-item/.test(classOf(el))
  const isActiveConversation = (el) => isConversationItem(el) && /active/.test(classOf(el))
  const extractAssetKey = (value) => {
    const text = String(value || '')
    const match = text.match(/\/([0-9a-f]{24,})(?:~tplv-[^/?#]+)?(?:[?#]|$)/i)
    return match ? match[1].toLowerCase() : ''
  }
  const findActiveConversation = () =>
    [...document.querySelectorAll('[class*="conversation-item"]')].find(isActiveConversation) ||
    null

  const markCurrentConversation = (taskToken) => {
    document
      .querySelectorAll(`[${conversationMarkerAttr}]`)
      .forEach((node) => node.removeAttribute(conversationMarkerAttr))

    const item = findActiveConversation()
    if (!item) {
      state.conversation_snapshot = { marked: false, reason: '未找到 active 会话' }
      return state.conversation_snapshot
    }

    const shell = item.closest?.('[class*="tooltip-trigger-shell"]') || item
    const title = textOf(item.querySelector?.('[class*="item-name"]') || item)
    const thumb = item.querySelector?.('img[src]')?.getAttribute('src') || ''
    const assetKey = extractAssetKey(thumb)
    const moreButton = item.querySelector?.('[class*="more-button"]')

    if (/默认创作/.test(title) || !moreButton) {
      state.conversation_snapshot = {
        marked: false,
        reason: !moreButton ? '当前会话没有更多菜单' : '默认创作不自动删除',
        title,
        assetKey,
      }
      return state.conversation_snapshot
    }

    shell.setAttribute(conversationMarkerAttr, taskToken)
    state.conversation_snapshot = {
      marked: true,
      title,
      assetKey,
      attr: conversationMarkerAttr,
      token: taskToken,
      at: Date.now(),
    }
    return state.conversation_snapshot
  }

  state.status = 'generating'
  state.task_token = nextTaskToken()
  state.submit_started_at = Date.now()
  state.submit_id = ''
  state.history_id = ''
  state.images = []
  state.imageCount = 0
  state.result_at = 0
  state.message = ''
  state.error_code = ''
  state.last_history_status = ''
  state.generate_request_count = 0
  state.generate_request_started_at = 0
  state.last_generate_request_url = ''
  state.submit_click_attempts = 0
  state.submit_confirmed = false
  state.submit_confirmation = null
  const conversation = markCurrentConversation(state.task_token)
  return JSON.stringify({
    marked: true,
    submit_started_at: state.submit_started_at,
    conversation,
  })
})()

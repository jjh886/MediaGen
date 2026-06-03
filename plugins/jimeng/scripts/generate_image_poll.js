// 即梦图片生成轮询：优先使用拦截器结果，降级 DOM 扫描。
;(() => {
  const SK = Symbol.for('_jimeng')
  const state = window[SK] || { status: 'generating', startTime: Date.now() }
  const now = Date.now()
  const submitAt = Number(state.submit_started_at || 0)
  const elapsed = now - (state.startTime || now)
  const submitElapsed = submitAt > 0 ? now - submitAt : 0
  const conversationMarkerAttr = 'data-mediagen-jimeng-task-conversation'

  const textOf = (el) => (el?.textContent || '').replace(/\s+/g, ' ').trim()
  const classOf = (el) => String(el?.className || '')
  const extractAssetKey = (value) => {
    const text = String(value || '')
    const match = text.match(/(?:^|\/)([0-9a-f]{24,})(?=$|[~/?#.:_-])/i)
    return match ? match[1].toLowerCase() : ''
  }

  const keepExistingMarkedSnapshot = (token) =>
    state.conversation_snapshot?.marked && state.conversation_snapshot?.token === token

  const activeConversationItems = () =>
    [...document.querySelectorAll('[class*="conversation-item"]')].filter((node) =>
      /active/.test(classOf(node))
    )

  const unmarkedSnapshot = (reason, extra = {}) => {
    const snapshot = { marked: false, reason, ...extra }
    state.conversation_snapshot = state.conversation_snapshot?.marked
      ? state.conversation_snapshot
      : snapshot
    return snapshot
  }

  const clearOtherConversationMarkers = (shell) => {
    document.querySelectorAll(`[${conversationMarkerAttr}]`).forEach((node) => {
      if (node !== shell) node.removeAttribute(conversationMarkerAttr)
    })
  }

  const markConversationSnapshot = (shell, token, title, assetKey) => {
    clearOtherConversationMarkers(shell)
    shell.setAttribute(conversationMarkerAttr, token)
    state.conversation_snapshot = {
      marked: true,
      title,
      assetKey,
      attr: conversationMarkerAttr,
      token,
      submit_id: state.submit_id,
      source: 'poll_submit_id',
      at: Date.now(),
    }
    return state.conversation_snapshot
  }

  const activeConversationDetails = (item) => {
    const shell = item.closest?.('[class*="tooltip-trigger-shell"]') || item
    const title = textOf(item.querySelector?.('[class*="item-name"]') || item)
    const moreButton = item.querySelector?.('[class*="more-button"]')
    const thumb =
      item.querySelector?.('img')?.currentSrc ||
      item.querySelector?.('img')?.getAttribute?.('src') ||
      ''
    return { shell, title, moreButton, assetKey: extractAssetKey(thumb) }
  }

  const unmarkableConversationReason = ({ title, moreButton }) => {
    if (!moreButton) return '当前会话没有更多菜单'
    if (/默认创作/.test(title)) return '默认创作不自动删除'
    return ''
  }

  const bindActiveConversation = () => {
    const token = state.task_token || ''
    if (!token) return { marked: false, reason: '缺少 task token' }
    if (!state.submit_id) return { marked: false, reason: '缺少 submit_id' }
    if (keepExistingMarkedSnapshot(token)) {
      return state.conversation_snapshot
    }

    const activeItems = activeConversationItems()
    if (activeItems.length !== 1) {
      const reason =
        activeItems.length === 0
          ? '未找到 active 会话'
          : `active 会话数量异常: ${activeItems.length}`
      return unmarkedSnapshot(reason)
    }

    const details = activeConversationDetails(activeItems[0])
    const reason = unmarkableConversationReason(details)
    if (reason) {
      return unmarkedSnapshot(reason, {
        title: details.title,
        submit_id: state.submit_id,
      })
    }

    return markConversationSnapshot(details.shell, token, details.title, details.assetKey)
  }

  const clickDialogAction = (preferClose = false) => {
    const buttons = [...document.querySelectorAll('button, [role="button"]')]
    const closeLike = buttons.find((node) => {
      const text = node.textContent || ''
      const aria = node.getAttribute?.('aria-label') || node.getAttribute?.('title') || ''
      return /关闭|close|×|x/i.test(`${text} ${aria}`.trim())
    })
    const confirmLike = buttons.find((node) =>
      /明白了|知道了|确定|OK|升级会员得积分/i.test(node.textContent || '')
    )
    const btn = preferClose ? closeLike || confirmLike : confirmLike || closeLike
    if (btn) {
      try {
        btn.click()
      } catch (_) {}
    }
  }

  const visibleDialogTexts = [
    ...document.querySelectorAll(
      '[role="dialog"], [aria-modal="true"], .lv-modal, .lv-dialog, .lv-modal-content'
    ),
  ]
    .map((node) => node.innerText || node.textContent || '')
    .filter(Boolean)
  const pageText = document.body?.innerText || document.body?.textContent || ''
  const bodyText = [...visibleDialogTexts, pageText].join('\n')

  const rateLimitHit =
    /rate_limit_exceeded|generating images too quickly|请求过于频繁|生成过于频繁|请稍等.*分钟/.test(
      bodyText
    )
  if (rateLimitHit) {
    clickDialogAction()
    return JSON.stringify({
      status: 'error',
      error_code: 'rate_limit_exceeded',
      message: bodyText.replace(/\s+/g, ' ').trim().slice(0, 500),
      retryable: true,
    })
  }

  const quotaHit =
    /积分不足|没有相关权益|没有足够的积分|积分.*不足|权益不足|请升级会员获得更多积分/.test(bodyText)
  if (quotaHit) {
    clickDialogAction(true)
    return JSON.stringify({
      status: 'error',
      error_code: 'quota_or_entitlement_insufficient',
      // 兼容账号管理器已有的“额度耗尽”分类规则，避免插件侧权益不足被当作普通流程失败。
      message: `额度耗尽: ${bodyText.replace(/\s+/g, ' ').trim()}`.slice(0, 500),
      retryable: false,
    })
  }

  if (/\/ai-tool\/work-detail\//.test(location.pathname) && !state.submit_id) {
    return JSON.stringify({
      status: 'error',
      error_code: 'jimeng_wrong_page_after_submit',
      message: `提交后页面离开生成页，可能点击到了作品详情: ${location.href}`,
      retryable: true,
    })
  }

  if (state.status === 'error') {
    return JSON.stringify({
      status: 'error',
      error_code: state.error_code || '',
      message: state.message || '即梦生成失败',
      retryable: /频繁|rate|繁忙/.test(state.message || ''),
    })
  }

  const conversation = bindActiveConversation()

  const submitStillPending = !state.submit_id && submitElapsed > 90000
  const submitRequestStarted =
    !!state.submit_id ||
    Number(state.generate_request_count || 0) > 0 ||
    (Array.isArray(state.fetchLog) &&
      state.fetchLog.some((item) => String(item?.url || '').includes('/aigc_draft/generate')))

  const hasFreshHistoryResult =
    state.status === 'done' &&
    Array.isArray(state.images) &&
    state.images.length > 0 &&
    submitAt > 0 &&
    !!state.submit_id &&
    Number(state.result_at || 0) >= submitAt &&
    state.source === 'jimeng_history'

  if (hasFreshHistoryResult) {
    return JSON.stringify({
      status: 'done',
      imageCount: state.images.length,
      images: state.images,
      source: state.source || 'jimeng_history',
      submit_id: state.submit_id || '',
      conversation,
    })
  }

  const fatalPhrases = ['无法生成', '违反', '不符合', '内容政策', '审核未通过']
  for (const phrase of fatalPhrases) {
    if (bodyText.includes(phrase)) {
      return JSON.stringify({
        status: 'error',
        message: `内容审核拦截: ${phrase}`,
        retryable: false,
      })
    }
  }
  if (elapsed > 600000) {
    return JSON.stringify({ status: 'error', message: '生成超时（10min）' })
  }

  return JSON.stringify({
    status: 'generating',
    elapsed: Math.round(elapsed / 1000),
    submit_id: state.submit_id || '',
    history_status: state.last_history_status || '',
    uploaded_refs: Array.isArray(state.uploaded_refs) ? state.uploaded_refs.length : 0,
    fetches: Array.isArray(state.fetchLog) ? state.fetchLog.length : 0,
    submit_elapsed: submitElapsed > 0 ? Math.round(submitElapsed / 1000) : 0,
    submit_pending_long: submitStillPending,
    submit_request_started: submitRequestStarted,
    submit_click_attempts: Number(state.submit_click_attempts || 0),
    generate_request_count: Number(state.generate_request_count || 0),
    diagnostic: submitStillPending
      ? submitRequestStarted
        ? `即梦提交后 ${Math.round(submitElapsed / 1000)} 秒仍未返回 submit_id，但已捕获提交请求，继续等待平台返回`
        : `即梦提交后 ${Math.round(submitElapsed / 1000)} 秒仍未返回 submit_id，未捕获提交请求，已重试点击 ${Number(state.submit_click_attempts || 0)} 次，继续等待平台自恢复`
      : '',
    conversation,
  })
})()

// 生成完成并下载结果后，隐藏本次图片生成产生的 ChatGPT 对话。
// ChatGPT 网页端删除对话实际调用 PATCH /backend-api/conversation/{id}，body 为 { is_visible: false }。
;(async () => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || {}
  const conversationId = String(state.result_conversation_id || state.conversation_id || '').trim()

  if (!conversationId) {
    return JSON.stringify({
      deleted: false,
      skipped: true,
      reason: 'missing_conversation_id',
    })
  }

  try {
    const resp = await fetch(`/backend-api/conversation/${encodeURIComponent(conversationId)}`, {
      method: 'PATCH',
      credentials: 'include',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ is_visible: false }),
    })

    let body = null
    try {
      body = await resp.json()
    } catch (_) {}

    if (!resp.ok) {
      return JSON.stringify({
        deleted: false,
        skipped: false,
        conversation_id: conversationId,
        status: resp.status,
        error: body?.detail || body?.error || resp.statusText || 'delete_failed',
      })
    }

    state.deleted_conversation_id = conversationId
    if (state.result_conversation_id === conversationId) state.result_conversation_id = ''
    if (state.conversation_id === conversationId) state.conversation_id = ''

    if (location.pathname === `/c/${conversationId}`) {
      history.replaceState(null, '', '/')
    }

    return JSON.stringify({
      deleted: true,
      skipped: false,
      conversation_id: conversationId,
      status: resp.status,
    })
  } catch (e) {
    return JSON.stringify({
      deleted: false,
      skipped: false,
      conversation_id: conversationId,
      error: String(e?.message || e),
    })
  }
})()

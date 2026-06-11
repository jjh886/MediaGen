// 生成结果下载完成后，按 ChatGPT 网页端的真实接口隐藏本次图片生成对话。
// 注意：ChatGPT 的“删除”是软删除，网页端实际发送 PATCH { is_visible: false }。
const KNOWN_API_HEADERS = [
  'oai-client-build-number',
  'oai-client-version',
  'oai-device-id',
  'oai-language',
  'oai-session-id',
]
const VERIFY_OFFSETS = [0, 28, 56, 84, 112]

function json(payload) {
  return JSON.stringify(payload)
}

function resolveConversationId(state) {
  return firstText([state.result_conversation_id, state.conversation_id, conversationIdFromPath()])
}

function conversationIdFromPath() {
  const match = String(location.pathname || '').match(/^\/c\/([^/?#]+)/)
  return match ? decodeURIComponent(match[1]) : ''
}

function firstText(values) {
  for (const value of values) {
    const text = String(value || '').trim()
    if (text) return text
  }
  return ''
}

async function hideConversation(conversationId, state) {
  const resp = await fetch(`/backend-api/conversation/${encodeURIComponent(conversationId)}`, {
    method: 'PATCH',
    credentials: 'include',
    headers: patchHeaders(conversationId, state),
    body: JSON.stringify({ is_visible: false }),
  })
  const body = await safeJson(resp)

  if (!resp.ok) return { ok: false, status: resp.status, error: errorText(body, resp) }
  if (body?.success === false)
    return { ok: false, status: resp.status, error: errorText(body, resp) }
  return { ok: true, status: resp.status }
}

async function verifyConversationHidden(conversationId, state) {
  const pages = []
  let verified = false

  for (const offset of VERIFY_OFFSETS) {
    const resp = await fetch(conversationsUrl(offset), {
      credentials: 'include',
      headers: baseHeaders(state, '/backend-api/conversations', '/backend-api/conversations'),
    })
    const body = await safeJson(resp)
    const contains = responseContainsConversation(body, conversationId)
    pages.push({ offset, status: resp.status, contains })
    if (resp.ok) verified = true
    if (contains) return { hidden: false, verified, pages }
  }

  return { hidden: verified, verified, pages }
}

async function initEmptyComposer(state) {
  try {
    await fetch('/backend-api/conversation/init', {
      method: 'POST',
      credentials: 'include',
      headers: initHeaders(state),
      body: JSON.stringify({
        requested_default_model: null,
        conversation_id: null,
        timezone_offset_min: new Date().getTimezoneOffset(),
      }),
    })
  } catch (e) {
    state.cleanup_init_error = String(e?.message || e)
  }
}

async function safeJson(resp) {
  try {
    return await resp.json()
  } catch (_) {
    return null
  }
}

function responseContainsConversation(body, conversationId) {
  if (!body) return false
  return JSON.stringify(body).includes(conversationId)
}

function patchHeaders(conversationId, state) {
  const headers = baseHeaders(
    state,
    `/backend-api/conversation/${conversationId}`,
    '/backend-api/conversation/{conversation_id}'
  )
  headers['content-type'] = 'application/json'
  return headers
}

function initHeaders(state) {
  const headers = baseHeaders(
    state,
    '/backend-api/conversation/init',
    '/backend-api/conversation/init'
  )
  headers['content-type'] = 'application/json'
  return headers
}

function baseHeaders(state, targetPath, targetRoute) {
  const saved = normalizeHeaders(state.api_headers)
  const headers = {
    accept: '*/*',
    'oai-language': firstText([saved['oai-language'], navigator.language, 'zh-CN']),
    'x-openai-target-path': targetPath,
    'x-openai-target-route': targetRoute,
  }
  copyKnownHeaders(headers, saved)
  return headers
}

function normalizeHeaders(headers) {
  const result = {}
  if (!headers) return result

  for (const [key, value] of Object.entries(headers)) {
    result[String(key).toLowerCase()] = String(value)
  }
  return result
}

function copyKnownHeaders(target, source) {
  for (const name of KNOWN_API_HEADERS) {
    const value = firstText([source[name]])
    if (value) target[name] = value
  }
}

function routeAwayFromConversation(conversationId) {
  if (location.pathname !== `/c/${conversationId}`) return
  history.replaceState(null, '', '/')
  window.dispatchEvent(new Event('popstate'))
}

function clearRuntimeConversation(state, conversationId) {
  state.deleted_conversation_id = conversationId
  if (state.result_conversation_id === conversationId) state.result_conversation_id = ''
  if (state.conversation_id === conversationId) state.conversation_id = ''
}

function failedResult(conversationId, patch) {
  return {
    deleted: false,
    skipped: false,
    verified: false,
    conversation_id: conversationId,
    status: patch.status,
    error: patch.error || 'delete_failed',
  }
}

function visibleResult(conversationId, patch, check) {
  return {
    deleted: false,
    skipped: false,
    verified: check.verified,
    conversation_id: conversationId,
    status: patch.status,
    error: check.verified ? 'conversation_still_visible' : 'conversation_list_refresh_failed',
  }
}

function conversationsUrl(offset) {
  return `/backend-api/conversations?offset=${offset}&limit=28&order=updated&is_archived=false&is_starred=false`
}

function errorText(body, resp) {
  return firstText([body?.detail, body?.error, resp?.statusText, 'delete_failed'])
}

;(async () => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || {}
  const conversationId = resolveConversationId(state)

  if (!conversationId)
    return json({ deleted: false, skipped: true, reason: 'missing_conversation_id' })

  try {
    const patch = await hideConversation(conversationId, state)
    if (!patch.ok) return json(failedResult(conversationId, patch))

    routeAwayFromConversation(conversationId)
    await initEmptyComposer(state)

    const check = await verifyConversationHidden(conversationId, state)
    if (!check.hidden) return json(visibleResult(conversationId, patch, check))

    clearRuntimeConversation(state, conversationId)
    return json({
      deleted: true,
      skipped: false,
      verified: check.verified,
      conversation_id: conversationId,
      status: patch.status,
    })
  } catch (e) {
    return json({
      deleted: false,
      skipped: false,
      conversation_id: conversationId,
      error: String(e?.message || e),
    })
  }
})()

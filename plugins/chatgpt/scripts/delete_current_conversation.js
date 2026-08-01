// 生成结果下载完成后，按 ChatGPT 网页端的真实接口隐藏本次图片生成对话。
// 注意：ChatGPT 的“删除”是软删除，网页端实际发送 PATCH { is_visible: false }。
const KNOWN_API_HEADERS = [
  'authorization',
  'chatgpt-account-id',
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
  // HAR 里的删除对象就是当前 /c/{id} 页面和侧边栏 active 行，优先使用真实页面状态。
  return firstText([
    conversationIdFromPath(),
    activeConversationIdFromDom(),
    state.result_conversation_id,
    state.conversation_id,
  ])
}

function conversationIdFromPath() {
  return conversationIdFromUrl(location.pathname)
}

function activeConversationIdFromDom() {
  const active =
    document.querySelector('a[data-sidebar-item][data-active][href*="/c/"]') ||
    document.querySelector('a[data-active][href*="/c/"]')
  return conversationIdFromUrl(active?.getAttribute('href') || '')
}

function conversationIdFromUrl(value) {
  const match = String(value || '').match(/\/c\/([^/?#]+)/)
  return match ? decodeURIComponent(match[1]) : ''
}

function firstText(values) {
  for (const value of values) {
    const text = valueText(value)
    if (text) return text
  }
  return ''
}

function valueText(value) {
  const text = scalarText(value)
  if (text) return text
  return objectText(value)
}

function scalarText(value) {
  if (value === null || value === undefined) return ''
  if (typeof value === 'string') return value.trim()
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  return ''
}

function objectText(value) {
  if (!value || typeof value !== 'object') return ''
  const known = firstText([value.message, value.detail, value.code, value.type])
  if (known) return known
  try {
    return JSON.stringify(value).slice(0, 500)
  } catch (_) {
    return 'unknown_object_error'
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function apiFetch(state, url, options = {}) {
  return typeof state.authenticatedFetch === 'function'
    ? state.authenticatedFetch(url, options)
    : fetch(url, options)
}

async function hideConversation(conversationId, state) {
  const resp = await apiFetch(
    state,
    `/backend-api/conversation/${encodeURIComponent(conversationId)}`,
    {
      method: 'PATCH',
      credentials: 'include',
      cache: 'no-store',
      referrer: `${location.origin}/c/${encodeURIComponent(conversationId)}`,
      headers: patchHeaders(conversationId, state),
      body: JSON.stringify({ is_visible: false }),
    }
  )
  const body = await safeJson(resp)
  const bodyError = responseErrorText(body, resp)

  if (!resp.ok) return { ok: false, status: resp.status, error: bodyError }
  if (bodyError) return { ok: false, status: resp.status, error: bodyError }
  return { ok: true, status: resp.status }
}

async function verifyConversationHidden(conversationId, state) {
  const pages = []
  let verified = false

  for (const offset of VERIFY_OFFSETS) {
    const resp = await apiFetch(state, conversationsUrl(offset), {
      credentials: 'include',
      cache: 'no-store',
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
    await apiFetch(state, '/backend-api/conversation/init', {
      method: 'POST',
      credentials: 'include',
      cache: 'no-store',
      headers: initHeaders(state),
      body: JSON.stringify({
        requested_default_model: null,
        conversation_id: null,
        timezone_offset_min: new Date().getTimezoneOffset(),
      }),
    })
  } catch (e) {
    state.cleanup_init_error = valueText(e)
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

function findOptionsButton(conversationId) {
  return [...document.querySelectorAll('[data-conversation-options-trigger]')].find(
    (button) => button.getAttribute('data-conversation-options-trigger') === conversationId
  )
}

function elementText(el) {
  return String(el?.innerText || el?.textContent || el?.getAttribute?.('aria-label') || '')
    .replace(/\s+/g, ' ')
    .trim()
}

function isVisibleElement(el) {
  const rect = el?.getBoundingClientRect?.()
  return !!rect && rect.width > 0 && rect.height > 0
}

function findDeleteMenuItem() {
  const controls = document.querySelectorAll('[role="menuitem"], [role="button"], button')
  return [...controls].find((el) => isVisibleElement(el) && /删除|Delete/i.test(elementText(el)))
}

function findConfirmDeleteButton() {
  const dialogs = document.querySelectorAll('[role="dialog"], [aria-modal="true"]')
  for (const dialog of dialogs) {
    const buttons = dialog.querySelectorAll('button, [role="button"]')
    const target = [...buttons].find((button) => /删除|Delete/i.test(elementText(button)))
    if (target) return target
  }
  return null
}

async function waitForElement(find, timeoutMs = 2500) {
  const started = Date.now()
  while (Date.now() - started < timeoutMs) {
    const target = find()
    if (target) return target
    await sleep(100)
  }
  return null
}

async function deleteViaDomMenu(conversationId) {
  const trigger = findOptionsButton(conversationId)
  if (!trigger) return { attempted: false, ok: false, reason: 'options_trigger_not_found' }

  trigger.click()
  const item = await waitForElement(findDeleteMenuItem)
  if (!item) return { attempted: true, ok: false, reason: 'delete_menu_item_not_found' }

  item.click()
  const confirm = await waitForElement(findConfirmDeleteButton, 1200)
  if (confirm) confirm.click()

  await sleep(1200)
  return { attempted: true, ok: true, reason: '' }
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

function failedResult(conversationId, cleanupError, extra = {}) {
  return {
    deleted: false,
    skipped: false,
    verified: false,
    conversation_id: conversationId,
    ...extra,
    // 不能用顶层 error，否则通用 run_script 会把“清理失败”升级成“生成失败”。
    cleanup_error: cleanupError || 'delete_failed',
  }
}

function visibleResult(conversationId, check, extra = {}) {
  return {
    deleted: false,
    skipped: false,
    verified: check.verified,
    conversation_id: conversationId,
    ...extra,
    cleanup_error: check.verified
      ? 'conversation_still_visible'
      : 'conversation_list_refresh_failed',
  }
}

function conversationsUrl(offset) {
  return `/backend-api/conversations?offset=${offset}&limit=28&order=updated&is_archived=false&is_starred=false`
}

function errorText(body, resp) {
  return firstText([body?.detail, body?.error, resp?.statusText, 'delete_failed'])
}

function responseErrorText(body, resp) {
  if (body?.success === false) return errorText(body, resp)
  return firstText([body?.detail, body?.error])
}

;(async () => {
  const SK = Symbol.for('_cgpt')
  const state = window[SK] || {}
  const conversationId = resolveConversationId(state)

  if (!conversationId)
    return json({ deleted: false, skipped: true, reason: 'missing_conversation_id' })

  try {
    const uiDelete = await deleteViaDomMenu(conversationId)
    let check = { hidden: false, verified: false }

    if (uiDelete.ok) {
      routeAwayFromConversation(conversationId)
      await initEmptyComposer(state)
      check = await verifyConversationHidden(conversationId, state)
    }
    if (!check.hidden) {
      const patch = await hideConversation(conversationId, state)
      if (!patch.ok) {
        return json(
          failedResult(conversationId, patch.error, {
            status: patch.status,
            ui_delete: uiDelete,
          })
        )
      }
      routeAwayFromConversation(conversationId)
      await initEmptyComposer(state)
      check = await verifyConversationHidden(conversationId, state)
      if (!check.hidden) {
        return json(
          visibleResult(conversationId, check, {
            status: patch.status,
            ui_delete: uiDelete,
          })
        )
      }
    }

    clearRuntimeConversation(state, conversationId)
    return json({
      deleted: true,
      skipped: false,
      verified: check.verified,
      conversation_id: conversationId,
      ui_delete: uiDelete,
    })
  } catch (e) {
    return json(failedResult(conversationId, valueText(e)))
  }
})()

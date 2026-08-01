// ChatGPT 上传 / 发送 / 结果下载拦截器
// 目标：
// 1. 跟踪参考图上传完成；
// 2. 抓取 conversation_id / image_gen_task_id；
// 3. 拦截结果文件下载接口，拿到最终 download_url。
;(() => {
  const SK = Symbol.for('_cgpt')
  const state = (window[SK] = window[SK] || {})
  const nextTaskToken = () => `cgpt_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
  const resetTaskState = () => {
    state.fetchLog = []
    state.uploaded_refs = []
    state.debugEvents = []
    state.status = 'generating'
    state.startTime = Date.now()
    state.submit_started_at = 0
    state.task_token = nextTaskToken()
    state.download_url = ''
    state.generated_file_id = ''
    state.result_conversation_id = ''
    state.conversation_id = ''
    state.image_gen_task_id = ''
    state.async_status = null
    state.stream_status = ''
    state.message = ''
    state.result_at = 0
    state.pending_result = null
  }
  const isCurrentTask = (taskToken) => taskToken && taskToken === state.task_token
  const extractRateLimitMessage = (text) => {
    const normalized = String(text || '')
      .replace(/\s+/g, ' ')
      .trim()
    if (!normalized) return ''
    const matched =
      normalized.match(
        /you['’]re generating images too quickly\.[\s\S]*?please wait for \d+\s+minutes? before generating more images\.?/i
      ) ||
      normalized.match(/you['’]re generating images too quickly/i) ||
      normalized.match(/please wait for \d+\s+minutes? before generating more images\.?/i) ||
      normalized.match(/rate_limit_exceeded/i)
    return matched ? matched[0].trim().slice(0, 220) : ''
  }

  resetTaskState()

  if (state.fetchHooked) {
    return JSON.stringify({ hooked: true, reused: true })
  }

  const pushLog = (url, extra) => {
    state.fetchLog.push({
      url: String(url || '').substring(0, 240),
      time: Date.now(),
      ...(extra || {}),
    })
    if (state.fetchLog.length > 100) state.fetchLog = state.fetchLog.slice(-100)
  }

  const pushEvent = (kind, extra) => {
    state.debugEvents.push({ kind, time: Date.now(), ...(extra || {}) })
    if (state.debugEvents.length > 100) state.debugEvents = state.debugEvents.slice(-100)
  }

  const parseFileIdFromUrl = (url) => {
    const match = String(url || '').match(/\/backend-api\/files\/download\/(file_[a-zA-Z0-9]+)/)
    return match ? match[1] : ''
  }

  // 记录网页端真实请求头，后续轮询和删除会话时复用。
  // ChatGPT 的 backend-api 不只校验 Cookie，还要求 Bearer token 和账号 ID；漏掉它们会把
  // 已登录账号误判成 conversation_inaccessible。
  const TRACKED_API_HEADERS = [
    'authorization',
    'chatgpt-account-id',
    'oai-client-build-number',
    'oai-client-version',
    'oai-device-id',
    'oai-language',
    'oai-session-id',
  ]

  const toReadableHeaders = (headers) => {
    try {
      return headers ? new Headers(headers) : null
    } catch (_) {
      return null
    }
  }

  const rememberApiHeaders = (args) => {
    const headers = toReadableHeaders(args[1]?.headers || args[0]?.headers)
    if (!headers) return

    const saved = (state.api_headers = state.api_headers || {})
    for (const name of TRACKED_API_HEADERS) {
      const value = headers.get(name)
      if (value) saved[name] = value
    }
  }

  const mergeApiHeaders = (headers) => {
    const merged = new Headers(headers || {})
    for (const [name, value] of Object.entries(state.api_headers || {})) {
      if (value && !merged.has(name)) merged.set(name, String(value))
    }
    return merged
  }

  const ensureApiAuth = async () => {
    const saved = (state.api_headers = state.api_headers || {})
    if (saved.authorization && saved['chatgpt-account-id']) return

    // accessToken 会过期，因此只在内存中按需从官方 session 接口刷新，不写入页面存储。
    const response = await origFetch('/api/auth/session', {
      credentials: 'include',
      cache: 'no-store',
      headers: { accept: 'application/json' },
    })
    if (!response.ok) throw new Error(`ChatGPT session HTTP ${response.status}`)
    const session = await response.json()
    if (!session?.accessToken || !session?.account?.id) {
      throw new Error('ChatGPT session 缺少 accessToken 或 account.id')
    }
    saved.authorization = `Bearer ${session.accessToken}`
    saved['chatgpt-account-id'] = session.account.id
  }

  // 统一给插件自己的后台请求补齐登录头，避免每个轮询脚本各写一套鉴权逻辑。
  const authenticatedFetch = async (input, options = {}) => {
    await ensureApiAuth()
    return origFetch(input, { ...options, headers: mergeApiHeaders(options.headers) })
  }

  const parseConversationIdFromUrl = (url) => {
    try {
      return new URL(String(url || ''), location.origin).searchParams.get('conversation_id') || ''
    } catch (_) {
      const match = String(url || '').match(/[?&]conversation_id=([^&]+)/)
      return match ? decodeURIComponent(match[1]) : ''
    }
  }

  const promotePendingResultIfMatched = () => {
    const pending = state.pending_result
    if (!pending || !state.conversation_id) return false
    if (pending.conversation_id !== state.conversation_id) return false
    state.generated_file_id = pending.file_id
    state.download_url = pending.download_url
    state.result_conversation_id = pending.conversation_id
    state.status = 'done'
    state.result_at = pending.time || Date.now()
    state.pending_result = null
    pushEvent('generated_download_promoted', {
      fileId: state.generated_file_id,
      conversationId: state.result_conversation_id,
    })
    return true
  }

  const rememberGeneratedDownload = (url, data, taskToken) => {
    if (!isCurrentTask(taskToken)) return
    const fileId = parseFileIdFromUrl(url)
    const conversationId = parseConversationIdFromUrl(url)
    if (!fileId || !data?.download_url) return
    if (!Number(state.submit_started_at || 0)) {
      pushEvent('generated_download_ignored_before_submit', { fileId, conversationId })
      return
    }
    if (!conversationId) {
      pushEvent('generated_download_ignored_without_conversation', { fileId })
      return
    }
    if (state.conversation_id && conversationId !== state.conversation_id) {
      pushEvent('generated_download_ignored_mismatch', {
        fileId,
        conversationId,
        expectedConversationId: state.conversation_id,
      })
      return
    }
    if (!state.conversation_id) {
      state.pending_result = {
        file_id: fileId,
        conversation_id: conversationId,
        download_url: data.download_url,
        time: Date.now(),
      }
      pushEvent('generated_download_pending', { fileId, conversationId })
      return
    }
    state.generated_file_id = fileId
    state.download_url = data.download_url
    state.result_conversation_id = conversationId
    state.status = 'done'
    state.result_at = Date.now()
    pushEvent('generated_download', { fileId, conversationId })
  }

  const handleUploadProcessStream = async (resp, taskToken) => {
    try {
      const text = await resp.clone().text()
      if (!isCurrentTask(taskToken)) return
      const lines = text
        .split('\n')
        .map((line) => line.trim())
        .filter(Boolean)
      for (const line of lines) {
        const evt = JSON.parse(line)
        if (evt?.event === 'file.processing.completed' && evt?.file_id) {
          if (!state.uploaded_refs.includes(evt.file_id)) {
            state.uploaded_refs.push(evt.file_id)
            pushEvent('upload_completed', { fileId: evt.file_id })
          }
        }
      }
    } catch (e) {
      pushEvent('upload_process_parse_error', { error: String(e?.message || e) })
    }
  }

  const parseJsonLine = (line) => {
    try {
      return JSON.parse(line)
    } catch (_) {
      return null
    }
  }

  const rememberConversationId = (conversationId) => {
    if (!conversationId) return
    state.conversation_id = conversationId
    promotePendingResultIfMatched()
  }

  // SSE 会同时推会话 token 和消息 patch，拆开处理可以避免一个函数承担太多分支。
  const handleConversationEvent = (data) => {
    if (!data?.conversation_id) return false
    if (data.type === 'resume_conversation_token') {
      rememberConversationId(data.conversation_id)
      pushEvent('conversation_started', { conversationId: data.conversation_id })
      return true
    }
    if (data.type === 'conversation_async_status') {
      rememberConversationId(data.conversation_id)
      state.async_status = data.async_status
      return true
    }
    return false
  }

  const rememberPatchConversation = (patch) => {
    if (!patch?.v?.conversation_id) return
    if (state.conversation_id) return
    rememberConversationId(patch.v.conversation_id)
  }

  const rememberImageTask = (message) => {
    const taskId = message?.metadata?.image_gen_task_id
    if (!taskId) return
    state.image_gen_task_id = taskId
    pushEvent('image_gen_task', { taskId })
  }

  const messageText = (message) => {
    const parts = message?.content?.parts
    return Array.isArray(parts) ? parts.join('\n') : ''
  }

  const rememberTextError = (text) => {
    const rateLimitMessage = extractRateLimitMessage(text)
    if (rateLimitMessage) {
      state.status = 'error'
      state.error_code = 'rate_limit_exceeded'
      state.message = rateLimitMessage
      return
    }
    if (/something went wrong|出错|错误|无法生成|try again/i.test(text)) {
      state.status = 'error'
      state.message = text.substring(0, 200)
    }
  }

  const handleMessagePatch = (patch) => {
    const message = patch?.v?.message
    if (!message) return
    rememberPatchConversation(patch)
    rememberImageTask(message)
    // 用户提示词可能正常包含“错误、无法生成”等约束词，只允许平台回复触发失败。
    if (message?.author?.role !== 'user') {
      rememberTextError(messageText(message))
    }
  }

  const parseSseLine = (line, taskToken) => {
    if (!isCurrentTask(taskToken)) return
    if (!line) return
    const data = parseJsonLine(line)
    if (handleConversationEvent(data)) return
    handleMessagePatch(data)
  }

  const readSseStream = async (resp, taskToken) => {
    try {
      const reader = resp.clone().body?.getReader()
      if (!reader) return
      const decoder = new TextDecoder()
      let buffer = ''
      while (true) {
        const { done, value } = await reader.read()
        if (done) break
        buffer += decoder.decode(value, { stream: true })
        const chunks = buffer.split('\n')
        buffer = chunks.pop() || ''
        for (const rawLine of chunks) {
          const line = rawLine.trim()
          if (line.startsWith('data:')) {
            parseSseLine(line.slice(5).trim(), taskToken)
          }
        }
      }
    } catch (e) {
      pushEvent('conversation_stream_error', { error: String(e?.message || e) })
    }
  }

  const origFetch = window.fetch
  const origFetchToString = Function.prototype.toString.call(origFetch)
  window.fetch = async function (...args) {
    const url = typeof args[0] === 'string' ? args[0] : args[0]?.url || ''
    const taskToken = state.task_token
    pushLog(url)
    rememberApiHeaders(args)

    const resp = await origFetch.apply(this, args)

    if (url.includes('/backend-api/files/process_upload_stream')) {
      handleUploadProcessStream(resp, taskToken)
    }

    if (url.includes('/backend-api/f/conversation')) {
      readSseStream(resp, taskToken)
    }

    if (url.includes('/backend-api/files/download/') && url.includes('conversation_id=')) {
      try {
        const data = await resp.clone().json()
        rememberGeneratedDownload(url, data, taskToken)
      } catch (e) {
        pushEvent('generated_download_parse_error', { error: String(e?.message || e) })
      }
    }

    return resp
  }

  window.fetch.toString = () => origFetchToString
  Object.defineProperty(window.fetch, 'toString', { enumerable: false })

  state.authenticatedFetch = authenticatedFetch

  const origOpen = XMLHttpRequest.prototype.open
  const origSend = XMLHttpRequest.prototype.send
  const origOpenToString = Function.prototype.toString.call(origOpen)
  const origSendToString = Function.prototype.toString.call(origSend)

  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    this.__cgpt_url = url || ''
    return origOpen.call(this, method, url, ...rest)
  }

  XMLHttpRequest.prototype.send = function (...args) {
    const url = this.__cgpt_url || ''
    const taskToken = state.task_token
    pushLog(url, { xhr: true })

    if (url.includes('/backend-api/files/download/') && url.includes('conversation_id=')) {
      this.addEventListener('load', function () {
        try {
          const data = JSON.parse(this.responseText)
          rememberGeneratedDownload(url, data, taskToken)
        } catch (_) {}
      })
    }

    if (url.includes('/backend-api/files/process_upload_stream')) {
      this.addEventListener('load', function () {
        try {
          if (!isCurrentTask(taskToken)) return
          const lines = String(this.responseText || '')
            .split('\n')
            .map((line) => line.trim())
            .filter(Boolean)
          for (const line of lines) {
            const evt = JSON.parse(line)
            if (evt?.event === 'file.processing.completed' && evt?.file_id) {
              if (!state.uploaded_refs.includes(evt.file_id)) {
                state.uploaded_refs.push(evt.file_id)
              }
            }
          }
        } catch (_) {}
      })
    }

    return origSend.apply(this, args)
  }

  XMLHttpRequest.prototype.open.toString = () => origOpenToString
  XMLHttpRequest.prototype.send.toString = () => origSendToString
  Object.defineProperty(XMLHttpRequest.prototype.open, 'toString', { enumerable: false })
  Object.defineProperty(XMLHttpRequest.prototype.send, 'toString', { enumerable: false })

  state.fetchHooked = true
  return JSON.stringify({ hooked: true, reused: false })
})()

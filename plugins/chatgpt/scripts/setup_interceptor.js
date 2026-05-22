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

  const parseSseLine = (line, taskToken) => {
    if (!isCurrentTask(taskToken)) return
    if (!line) return
    try {
      const data = JSON.parse(line)
      if (data?.type === 'resume_conversation_token' && data?.conversation_id) {
        state.conversation_id = data.conversation_id
        pushEvent('conversation_started', { conversationId: data.conversation_id })
        promotePendingResultIfMatched()
        return
      }
      if (data?.type === 'conversation_async_status' && data?.conversation_id) {
        state.conversation_id = data.conversation_id
        state.async_status = data.async_status
        promotePendingResultIfMatched()
        return
      }
    } catch (_) {}

    try {
      const patch = JSON.parse(line)
      const message = patch?.v?.message
      if (!message) return
      if (patch?.v?.conversation_id && !state.conversation_id) {
        state.conversation_id = patch.v.conversation_id
        promotePendingResultIfMatched()
      }
      const meta = message?.metadata || {}
      if (meta?.image_gen_task_id) {
        state.image_gen_task_id = meta.image_gen_task_id
        pushEvent('image_gen_task', { taskId: meta.image_gen_task_id })
      }
      const textParts = message?.content?.parts || []
      const text = Array.isArray(textParts) ? textParts.join('\n') : ''
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
    } catch (_) {}
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

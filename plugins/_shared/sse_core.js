// 通用 SSE 拦截与解析内核
// ------------------------------------------------------------------
// 供各平台插件复用的 SSE 处理骨架，消除跨插件的 "hook window.fetch + 读 stream +
// 按事件派发" 重复代码。平台差异（URL 匹配、事件解析）通过 onEvent 回调注入。
//
// 使用方式：插件的入口脚本通过 manifest 的 `script_files` 数组把本文件排在业务
// 脚本之前一起加载，随后调用 `window.__installSseReader({ ... })`。
//
// 共享状态挂在 `window[Symbol.for(stateKey)]`（默认 '_s'），字段约定：
//   status          'generating' | 'done' | 'error' | 'needs_verification'
//   startTime       Date.now()
//   images          [{ url, preview_url, download_url, width, height, ... }]
//   expectedCount   number
//   _completedCount number
//   error_code      any
//   message         string
//   fetchHooked     true   // 避免重复注入
//   sseDebug        object // 诊断字段
//
// 入参 opts:
//   stateKey     : string，Symbol.for 的 key，默认 '_s'
//   urlPattern   : string，url.includes(pattern) 的子串匹配
//   onEvent(ev, payload, state)
//                  ev: SSE 事件名（不带 'event:' 前缀），可能为空串
//                  payload: SSE 'data:' 行原文（已 trim）
//                  state: 本次共享状态对象
;(() => {
  if (typeof window === 'undefined' || window.__installSseReader) return

  window.__installSseReader = function installSseReader(opts) {
    const stateKey = (opts && opts.stateKey) || '_s'
    const urlPattern = (opts && opts.urlPattern) || ''
    const onEvent = opts && typeof opts.onEvent === 'function' ? opts.onEvent : () => {}

    const SK = Symbol.for(stateKey)
    const s = (window[SK] = window[SK] || {})

    // 初始化每次任务的状态（保留 fetchHooked/downloadBlocked 等一次性标志）
    s.status = 'generating'
    s.startTime = Date.now()
    s.images = []
    s.expectedCount = 0
    s._completedCount = 0
    s.error_code = undefined
    s.message = undefined

    // 下载拦截兜底（跳转后 stealth 可能失效）
    if (!s.downloadBlocked) {
      const origClick = HTMLAnchorElement.prototype.click
      HTMLAnchorElement.prototype.click = function () {
        if (this.download || (this.href && this.href.startsWith('blob:'))) return
        return origClick.apply(this, arguments)
      }
      s.downloadBlocked = true
    }

    if (s.fetchHooked) {
      return { hooked: true, reused: true }
    }

    const origFetch = window.fetch
    const origFetchToString = Function.prototype.toString.call(origFetch)

    window.fetch = async function (...args) {
      const url = typeof args[0] === 'string' ? args[0] : args[0]?.url || ''
      const resp = await origFetch.apply(this, args)

      if (urlPattern && url.includes(urlPattern) && s.status === 'generating') {
        s.sseDebug = { matched: true, hasBody: !!resp.body, status: resp.status, time: Date.now() }
        try {
          const cloned = resp.clone()
          const reader = cloned.body?.getReader()
          if (reader) {
            s.sseDebug.readerOk = true
            readSseStream(reader, onEvent, s).catch((e) => {
              s.sseDebug.readError = String(e)
            })
          } else {
            s.sseDebug.readerOk = false
          }
        } catch (e) {
          s.sseDebug.cloneError = String(e)
        }
      }

      return resp
    }

    // 伪装 fetch 本体，避免 toString/name/length 暴露包装器
    Object.defineProperty(window.fetch, 'toString', {
      value: () => origFetchToString,
      writable: false,
      enumerable: false,
      configurable: true,
    })
    try {
      Object.defineProperty(window.fetch, 'name', { value: 'fetch', configurable: true })
    } catch (_) {}

    s.fetchHooked = true
    return { hooked: true }
  }

  function makeLineParser(onEvent, s) {
    let currentEvent = ''
    return (line) => {
      if (!line) return
      if (line.startsWith('event:')) {
        currentEvent = line.slice(6).trim()
        return
      }
      if (!line.startsWith('data:')) return
      const ev = currentEvent
      currentEvent = ''
      try {
        onEvent(ev, line.slice(5).trim(), s)
      } catch (_) {}
    }
  }

  function drainBuffer(buffer, text, parseLine) {
    const combined = buffer + text
    const lines = combined.split('\n')
    const tail = lines.pop() || ''
    for (const line of lines) parseLine(line)
    return tail
  }

  async function readSseStream(reader, onEvent, s) {
    const decoder = new TextDecoder()
    const parseLine = makeLineParser(onEvent, s)
    let buffer = ''
    let chunkCount = 0
    let totalBytes = 0

    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      chunkCount++
      totalBytes += value.byteLength
      const text = decoder.decode(value, { stream: true })
      s.sseDebug = { ...(s.sseDebug || {}), chunkCount, totalBytes, lastChunkLen: text.length }
      buffer = drainBuffer(buffer, text, parseLine)
    }

    if (buffer) parseLine(buffer)
    s.sseDebug = { ...(s.sseDebug || {}), streamDone: true, chunkCount, totalBytes }
  }

  // 导出独立解析器，供 trigger 自己 await fetch().body 时直接复用，
  // 避免"hook window.fetch + clone()"路径被站点自身 fetch wrapper 抢走 body
  window.__readSseStream = readSseStream
})()

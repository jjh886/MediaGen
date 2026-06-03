// 豆包 SSE 拦截器：从 chat/completion 的 SSE 流中提取生成结果
// SSE 关键事件: STREAM_CHUNK → block_type:2074 → creation_block → creations[]
// 每个 creation 有 status:1(生成中) → status:2(完成，含完整 URL)
// image_ori_raw.url 是无水印原图，可直接外部下载
//
// 所有状态挂在 window[Symbol.for('_s')] 下，而非可枚举的 __mediagen_* 前缀，
// 降低反爬 `Object.keys(window).filter(k => k.startsWith('__'))` 类特征扫描命中率。
;(() => {
  const SK = Symbol.for('_s')
  const s = (window[SK] = window[SK] || {})

  // 页面跳转后重新注入下载拦截（stealth 补丁可能在跳转间失效）
  if (!s.downloadBlocked) {
    const origClick = HTMLAnchorElement.prototype.click
    HTMLAnchorElement.prototype.click = function () {
      if (this.download || (this.href && this.href.startsWith('blob:'))) return
      return origClick.apply(this, arguments)
    }
    s.downloadBlocked = true
  }

  // 初始化状态
  s.status = 'generating'
  s.startTime = Date.now()
  s.images = []
  s.expectedCount = 0
  s._completedCount = 0
  s.error_code = undefined
  s.message = undefined

  function mapImageFields(img) {
    const raw = img.image_ori_raw
    const ph = img.placeholder
    return {
      url: raw.url,
      preview_url: img.image_preview?.url || '',
      download_url: img.image_ori?.url || '',
      thumb_url: img.image_thumb?.url || '',
      width: raw.width || ph?.width || 0,
      height: raw.height || ph?.height || 0,
    }
  }

  function extractCompletedImages(creations) {
    const completed = []
    for (const c of creations) {
      const img = c.image || {}
      if (img.status !== 2 || !img.image_ori_raw?.url) continue
      completed.push(mapImageFields(img))
    }
    return completed
  }

  function processCreationBlock(cb) {
    if (cb.block_type !== 2074 || !cb.content?.creation_block) return
    const creations = cb.content.creation_block.creations || []
    if (s.expectedCount === 0 && creations.length > 0) {
      s.expectedCount = creations.length
    }
    const completed = extractCompletedImages(creations)
    if (completed.length === 0) return
    s.images = completed
    s._completedCount = completed.length
    if (completed.length >= s.expectedCount) {
      s.status = 'done'
    }
  }

  // SSE 格式: "event: XXX\ndata: {...}\n\n"，需要按事件分组解析
  let _currentEvent = ''

  function handleStreamError(payload) {
    try {
      const d = JSON.parse(payload)
      const decision = d.extra?.decision ? JSON.parse(d.extra.decision) : {}
      const needsVerify = decision.type === 'verify' || d.error_msg?.includes('rate limit')
      s.status = needsVerify ? 'needs_verification' : 'error'
      s.error_code = d.error_code
      s.message = d.error_msg || 'STREAM_ERROR'
    } catch (_) {
      s.status = 'error'
      s.message = 'STREAM_ERROR (parse failed)'
    }
  }

  function handleStreamChunk(payload) {
    try {
      const d = JSON.parse(payload)
      for (const op of d.patch_op || []) {
        for (const cb of op.patch_value?.content_block || []) {
          processCreationBlock(cb)
        }
      }
    } catch (_) {}
  }

  function parseSseLine(line) {
    if (line.startsWith('event:')) {
      _currentEvent = line.substring(6).trim()
      return
    }
    if (!line.startsWith('data:')) return
    const event = _currentEvent
    _currentEvent = ''
    if (event === 'STREAM_ERROR') handleStreamError(line.substring(5).trim())
    else handleStreamChunk(line.substring(5).trim())
  }

  async function readSseStream(reader) {
    const decoder = new TextDecoder()
    let chunkCount = 0
    let totalBytes = 0
    let buffer = ''
    while (true) {
      const { done, value } = await reader.read()
      if (done) {
        if (buffer) parseSseLine(buffer)
        s.sseDebug = { ...(s.sseDebug || {}), streamDone: true, chunkCount, totalBytes }
        break
      }
      chunkCount++
      totalBytes += value.byteLength
      const text = decoder.decode(value, { stream: true })
      s.sseDebug = { ...(s.sseDebug || {}), chunkCount, totalBytes, lastChunkLen: text.length }
      const combined = buffer + text
      const lines = combined.split('\n')
      buffer = lines.pop() || ''
      for (const line of lines) {
        parseSseLine(line)
      }
    }
  }

  if (s.fetchHooked) {
    return JSON.stringify({ hooked: true, reused: true })
  }

  const origFetch = window.fetch
  const origFetchToString = Function.prototype.toString.call(origFetch)
  window.fetch = async function (...args) {
    const url = typeof args[0] === 'string' ? args[0] : args[0]?.url || ''

    if (!s.fetchLog) s.fetchLog = []
    s.fetchLog.push({ url: url.substring(0, 120), time: Date.now() })

    const resp = await origFetch.apply(this, args)

    if (url.includes('chat/completion') && s.status === 'generating') {
      s.sseDebug = { matched: true, hasBody: !!resp.body, status: resp.status, time: Date.now() }
      try {
        const cloned = resp.clone()
        const reader = cloned.body?.getReader()
        if (reader) {
          s.sseDebug.readerOk = true
          readSseStream(reader).catch((e) => {
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

  // 保护 fetch.toString() / .name / .length 返回原生签名
  const wrapperFetch = window.fetch
  Object.defineProperty(wrapperFetch, 'toString', {
    value: () => origFetchToString,
    writable: false,
    enumerable: false,
    configurable: true,
  })
  try {
    Object.defineProperty(wrapperFetch, 'name', { value: 'fetch', configurable: true })
  } catch (_) {}

  s.fetchHooked = true
  return JSON.stringify({ hooked: true })
})()

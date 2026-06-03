// Flow API 拦截器：捕获 batchGenerateImages 响应
// 同时拦截 fetch 和 XMLHttpRequest（防止 Flow 前端缓存了原始 fetch 引用）
;(() => {
  // 记录初始图片数量（DOM 扫描 baseline）
  const currentImgs = document.querySelectorAll(
    'img[alt="生成的图片"], img[alt="Generated image"], img[src*="getMediaUrlRedirect"]'
  )

  window.__mediagen = {
    status: 'generating',
    startTime: Date.now(),
    _initialImgCount: currentImgs.length,
    _initialAllImgCount: document.querySelectorAll(
      'img[src*="getMediaUrlRedirect"], img[src*="googleapis.com"]'
    ).length,
  }

  if (window.__mediagen_fetch_hooked) {
    return JSON.stringify({ hooked: true, reused: true, baseline: currentImgs.length })
  }

  // 初始化 fetch 日志（用于诊断）
  if (!window.__mediagen_fetch_log) window.__mediagen_fetch_log = []

  // 解析 batchGenerateImages 响应（支持多张图片）
  const parseGenerateResponse = (data) => {
    try {
      const mediaList = data?.media || []
      if (mediaList.length === 0) return false

      const images = []
      for (const media of mediaList) {
        const img = media?.image?.generatedImage
        if (img?.fifeUrl) {
          images.push({
            url: img.fifeUrl,
            name: media.name || '',
            workflowId: media.workflowId || '',
            width: media.image?.dimensions?.width || 0,
            height: media.image?.dimensions?.height || 0,
          })
        }
      }

      if (images.length > 0) {
        window.__mediagen = {
          ...window.__mediagen,
          status: 'done',
          url: images[0].url,
          name: images[0].name,
          workflowId: images[0].workflowId,
          width: images[0].width,
          height: images[0].height,
          images,
          imageCount: images.length,
        }
        return true
      }
    } catch (_) {}
    return false
  }

  // 检查响应是否包含错误
  const parseErrorResponse = (data) => {
    try {
      if (data?.error) {
        const reason = data.error.details?.[0]?.reason || data.error.message || ''
        if (reason.includes('UNSAFE')) {
          window.__mediagen = { status: 'error', message: '内容审核拦截: ' + reason }
        } else {
          window.__mediagen = { status: 'error', message: 'API 错误: ' + reason }
        }
        return true
      }
    } catch (_) {}
    return false
  }

  const isBatchGenerate = (url) =>
    url.includes('batchGenerateImages') || url.includes('flowMedia:batch')

  const isUploadImage = (url) => url.includes('uploadImage')

  // 初始化参考图追踪
  if (!window.__mediagen_uploaded_refs) window.__mediagen_uploaded_refs = []

  // ── 策略一：拦截 fetch ──
  const origFetch = window.fetch
  const origFetchToString = Function.prototype.toString.call(origFetch)
  window.fetch = async function (...args) {
    const url = typeof args[0] === 'string' ? args[0] : args[0]?.url || ''

    // 诊断日志
    window.__mediagen_fetch_log.push({ url: url.substring(0, 120), time: Date.now() })

    const resp = await origFetch.apply(this, args)

    // 拦截生成请求
    if (isBatchGenerate(url) && window.__mediagen?.status === 'generating') {
      try {
        const cloned = resp.clone()
        const data = await cloned.json()
        if (resp.ok) {
          parseGenerateResponse(data)
        } else {
          parseErrorResponse(data)
        }
      } catch (e) {}
    }

    // 拦截上传请求，保存 workflowId 供 cleanup 使用
    if (isUploadImage(url) && resp.ok) {
      try {
        const cloned = resp.clone()
        const data = await cloned.json()
        if (data?.media?.workflowId) {
          window.__mediagen_uploaded_refs.push({
            name: data.media.name,
            workflowId: data.media.workflowId,
          })
        }
      } catch (_) {}
    }

    return resp
  }

  // 保护 fetch.toString() 返回原生签名，防止反爬检测
  window.fetch.toString = () => origFetchToString
  Object.defineProperty(window.fetch, 'toString', { enumerable: false })

  // ── 策略二：拦截 XMLHttpRequest ──
  const origXHROpen = XMLHttpRequest.prototype.open
  const origXHRSend = XMLHttpRequest.prototype.send
  const origOpenToString = Function.prototype.toString.call(origXHROpen)
  const origSendToString = Function.prototype.toString.call(origXHRSend)
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    this.__mediagen_url = url || ''
    return origXHROpen.call(this, method, url, ...rest)
  }
  XMLHttpRequest.prototype.send = function (...args) {
    if (isBatchGenerate(this.__mediagen_url)) {
      this.addEventListener('load', function () {
        if (window.__mediagen?.status !== 'generating') return
        try {
          const data = JSON.parse(this.responseText)
          if (this.status >= 200 && this.status < 300) {
            parseGenerateResponse(data)
          } else {
            parseErrorResponse(data)
          }
        } catch (_) {}
      })
    }
    return origXHRSend.apply(this, args)
  }

  // 保护 XHR 方法的 toString()
  XMLHttpRequest.prototype.open.toString = () => origOpenToString
  XMLHttpRequest.prototype.send.toString = () => origSendToString
  Object.defineProperty(XMLHttpRequest.prototype.open, 'toString', { enumerable: false })
  Object.defineProperty(XMLHttpRequest.prototype.send, 'toString', { enumerable: false })

  window.__mediagen_fetch_hooked = true
  return JSON.stringify({ hooked: true, reused: false, baseline: currentImgs.length })
})()

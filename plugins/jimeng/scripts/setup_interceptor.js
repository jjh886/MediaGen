// 即梦 API 拦截器：捕获上传 uri、aigc_draft/generate 的 submit_id、
// get_history_by_ids 的生成图片 URL。状态存在 Symbol.for('_jimeng')。
;(() => {
  const SK = Symbol.for('_jimeng')
  const state = (window[SK] = window[SK] || {})
  const params = state.params || {}
  const nextTaskToken = () => `jimeng_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
  const isCurrentTask = (taskToken) => taskToken && taskToken === state.task_token
  const ratioMap = {
    '1:1': { ratio: 1, width: 2048, height: 2048 },
    '3:4': { ratio: 2, width: 1728, height: 2304 },
    '16:9': { ratio: 3, width: 2560, height: 1440 },
    '4:3': { ratio: 4, width: 2304, height: 1728 },
    '9:16': { ratio: 5, width: 1440, height: 2560 },
    '2:3': { ratio: 6, width: 1664, height: 2496 },
    '3:2': { ratio: 7, width: 2496, height: 1664 },
    '21:9': { ratio: 8, width: 3024, height: 1296 },
  }
  const models = {
    '图片5.0lite': { reqKey: 'high_aes_general_v50', name: '图片5.0 Lite' },
    '图片4.6': { reqKey: 'high_aes_general_v42', name: '图片4.6' },
    '图片4.5': { reqKey: 'high_aes_general_v40l', name: '图片 4.5' },
    '图片4.1': { reqKey: 'high_aes_general_v41', name: '图片 4.1' },
    '图片4.0': { reqKey: 'high_aes_general_v40', name: '图片 4.0' },
    '图片3.1': { reqKey: 'high_aes_general_v30l_art_fangzhou:general_v3.0_18b', name: '图片 3.1' },
    '图片3.0': { reqKey: 'high_aes_general_v30l:general_v3.0_18b', name: '图片 3.0' },
  }

  const currentImgs = [
    ...document.querySelectorAll('img[src*="dreamina-sign"][src*="aigc_resize"]'),
  ].map((img) => img.src)
  Object.assign(state, {
    status: 'generating',
    startTime: Date.now(),
    task_token: nextTaskToken(),
    submit_started_at: 0,
    submit_id: '',
    history_id: '',
    images: [],
    imageCount: 0,
    result_at: 0,
    message: '',
    error_code: '',
    uploaded_refs: [],
    uploadLog: [],
    fetchLog: [],
    generate_request_count: 0,
    generate_request_started_at: 0,
    last_generate_request_url: '',
    submit_click_attempts: 0,
    submit_confirmed: false,
    submit_confirmation: null,
    initialImageUrls: currentImgs,
    params,
  })

  const addUnique = (arr, value) => {
    if (!value || arr.includes(value)) return
    arr.push(value)
  }

  const safeJson = (text) => {
    if (!text || typeof text !== 'string') return null
    try {
      return JSON.parse(text)
    } catch (_) {
      return null
    }
  }

  const normalizeModelName = (value) => {
    let text = String(value || '').trim()
    text = text.replace(/^jimeng[_\s-]*/i, '')
    text = text.replace(/_/g, ' ')
    text = text.replace(/图片\s*([0-9])/g, '图片$1')
    return text.replace(/\s+/g, '').toLowerCase()
  }

  const selectedModel = () => {
    const raw = state.params?.modelDisplayName || state.params?.modelId || ''
    const key = normalizeModelName(raw)
    return models[key] || models['图片4.1']
  }

  const selectedRatio = () => {
    const raw = String(state.params?.ratio || '9:16').trim()
    return ratioMap[raw] || ratioMap['9:16']
  }

  const finalPrompt = () => {
    const prompt = String(state.params?.prompt || '').trim()
    const ratio = String(state.params?.ratio || '9:16').trim() || '9:16'
    if (!prompt) return ''
    return prompt.includes(ratio) ? prompt : `${prompt}\n画面比例：${ratio}`
  }

  const patchCoreParam = (core, model, ratio) => {
    if (!core || typeof core !== 'object') return false
    let changed = false
    if ('model' in core) {
      core.model = model.reqKey
      changed = true
    }
    if ('prompt' in core) {
      const prompt = finalPrompt()
      if (prompt) core.prompt = prompt
      changed = true
    }
    if ('image_ratio' in core || 'large_image_info' in core || 'intelligent_ratio' in core) {
      core.image_ratio = ratio.ratio
      core.intelligent_ratio = false
      core.large_image_info = {
        ...(core.large_image_info || {}),
        height: ratio.height,
        width: ratio.width,
        resolution_type: '2k',
      }
      changed = true
    }
    return changed
  }

  const patchDraftNode = (node, model, ratio) => {
    if (!node || typeof node !== 'object') return 0
    let count = 0
    if (patchCoreParam(node.core_param, model, ratio)) count += 1
    for (const value of Object.values(node)) {
      if (Array.isArray(value)) {
        for (const item of value) count += patchDraftNode(item, model, ratio)
      } else if (value && typeof value === 'object') {
        count += patchDraftNode(value, model, ratio)
      }
    }
    return count
  }

  const patchMetricsExtra = (text, model, ratio) => {
    const metrics = safeJson(text)
    if (!metrics || typeof metrics !== 'object') return text
    try {
      const sceneOptions = safeJson(metrics.sceneOptions)
      if (Array.isArray(sceneOptions)) {
        for (const option of sceneOptions) {
          if (!option || typeof option !== 'object') continue
          option.modelReqKey = model.reqKey
          option.resolutionType = '2k'
          option.benefitCount = 4
          option.reportParams = option.reportParams || {}
          option.reportParams.extraVipFunctionKey = `${model.reqKey}-2k`
        }
        metrics.sceneOptions = JSON.stringify(sceneOptions)
      }
      metrics.generateCount = Number(metrics.generateCount || 1)
      metrics.isRegenerate = false
      return JSON.stringify(metrics)
    } catch (_) {
      return text
    }
  }

  const patchGenerateBodyText = (url, bodyText) => {
    if (!url.includes('/aigc_draft/generate')) return bodyText
    const body = safeJson(bodyText)
    if (!body || typeof body !== 'object') return bodyText

    const model = selectedModel()
    const ratio = selectedRatio()
    body.extend = body.extend || {}
    body.extend.root_model = model.reqKey

    let patchedCores = 0
    const draft = safeJson(body.draft_content)
    if (draft) {
      patchedCores = patchDraftNode(draft, model, ratio)
      body.draft_content = JSON.stringify(draft)
    }
    if (typeof body.metrics_extra === 'string') {
      body.metrics_extra = patchMetricsExtra(body.metrics_extra, model, ratio)
    }
    state.generatePatch = {
      at: Date.now(),
      model: model.reqKey,
      modelName: model.name,
      ratio: ratio.ratio,
      width: ratio.width,
      height: ratio.height,
      patchedCores,
    }
    return JSON.stringify(body)
  }

  const readBody = async (input, init) => {
    const body = init?.body
    if (typeof body === 'string') return body
    if (body instanceof URLSearchParams) return body.toString()
    if (input && typeof input !== 'string' && typeof input.clone === 'function') {
      try {
        return await input.clone().text()
      } catch (_) {}
    }
    return ''
  }

  const toUrl = (input) => (typeof input === 'string' ? input : input?.url || '')

  const disguiseFetch = (fetchFn, nativeSource) => {
    Object.defineProperty(fetchFn, 'toString', {
      value() {
        return nativeSource
      },
      writable: false,
      enumerable: false,
      configurable: true,
    })
    try {
      Object.defineProperty(fetchFn, 'name', { value: 'fetch', configurable: true })
    } catch (_) {}
  }

  const normalizeImage = (item) => {
    const large = item?.image?.large_images?.[0] || {}
    const common = item?.common_attr || {}
    const url = large.image_url || common.cover_url || ''
    if (!url) return null
    return {
      url,
      width: Number(large.width || 0),
      height: Number(large.height || 0),
      image_uri: large.image_uri || common.cover_uri || common.id || '',
    }
  }

  const historyEntries = (data) => Object.entries(data?.data || {})

  const shouldUseHistoryRecord = (submitId) => !state.submit_id || submitId === state.submit_id

  const updateHistoryState = (record) => {
    const task = record?.task || {}
    state.last_history_status = String(task.status || '')
    state.history_id = record?.history_record_id || state.history_id || ''
  }

  const pushUniqueImage = (out, img) => {
    if (!img) return
    const exists = out.some(
      (x) => (x.image_uri && x.image_uri === img.image_uri) || x.url === img.url
    )
    if (!exists) out.push(img)
  }

  const extractImagesFromHistory = (data) => {
    const out = []
    for (const [submitId, record] of historyEntries(data)) {
      if (!shouldUseHistoryRecord(submitId)) continue
      updateHistoryState(record)

      const items = Array.isArray(record?.item_list) ? record.item_list : []
      for (const item of items) {
        pushUniqueImage(out, normalizeImage(item))
      }
    }
    return out
  }

  const uploadRefsFromPair = (url, body, resp) => {
    const refs = []
    if (url.includes('/imagex/submit_audit_job')) {
      refs.push(...(body?.uri_list || []))
    }
    if (url.includes('/algo_proxy')) {
      refs.push(...(body?.file_list || []).map((f) => f?.file_uri))
      refs.push(...(resp?.data?.file_list || []).map((f) => f?.file_uri || f?.uri))
    }
    return refs
  }

  const rememberUploadRefs = (refs, url) => {
    for (const uri of refs) {
      addUnique(state.uploaded_refs, uri)
      state.uploadLog.push({ uri, at: Date.now(), source: url.split('?')[0] })
    }
  }

  const captureUploadRefs = (url, bodyText, responseText, taskToken) => {
    if (!isCurrentTask(taskToken)) return
    const body = safeJson(bodyText)
    const resp = safeJson(responseText)
    rememberUploadRefs(uploadRefsFromPair(url, body, resp), url)
  }

  const isGenerateUrl = (url) => url.includes('/aigc_draft/generate')

  const updateGenerateIds = (body, resp) => {
    const submitId = body.submit_id || resp?.data?.aigc_data?.task?.submit_id || ''
    if (submitId) state.submit_id = submitId
    state.history_id = resp?.data?.aigc_data?.history_record_id || state.history_id || ''
  }

  const recordResponseError = (resp, fallback) => {
    if (!resp.ret || resp.ret === '0') return false
    state.status = 'error'
    state.error_code = resp.ret
    state.message = resp.errmsg || fallback
    return true
  }

  const captureGenerate = (url, bodyText, responseText, taskToken) => {
    if (!isCurrentTask(taskToken)) return
    if (!isGenerateUrl(url)) return
    const body = safeJson(bodyText) || {}
    const resp = safeJson(responseText) || {}
    updateGenerateIds(body, resp)
    recordResponseError(resp, '即梦生成提交失败')
  }

  const noteGenerateRequest = (url, taskToken) => {
    if (!isCurrentTask(taskToken)) return
    if (!isGenerateUrl(url)) return
    state.generate_request_count = Number(state.generate_request_count || 0) + 1
    state.generate_request_started_at = Date.now()
    state.last_generate_request_url = String(url).slice(0, 160)
  }

  const shouldCaptureHistory = (url, body) => {
    if (!url.includes('/get_history_by_ids')) return false
    if (!state.submit_started_at || !state.submit_id) return false
    const bodySubmitIds = Array.isArray(body.submit_ids) ? body.submit_ids : []
    return bodySubmitIds.length === 0 || bodySubmitIds.includes(state.submit_id)
  }

  const applyHistoryImages = (images) => {
    if (images.length === 0) return
    state.images = images
    state.imageCount = images.length
    state.status = 'done'
    state.result_at = Date.now()
    state.source = 'jimeng_history'
  }

  const captureHistory = (url, bodyText, responseText, taskToken) => {
    if (!isCurrentTask(taskToken)) return
    const body = safeJson(bodyText) || {}
    if (!shouldCaptureHistory(url, body)) return
    const resp = safeJson(responseText) || {}
    if (recordResponseError(resp, '即梦轮询失败')) return
    applyHistoryImages(extractImagesFromHistory(resp))
  }

  const processPair = (url, bodyText, responseText, taskToken) => {
    if (!isCurrentTask(taskToken)) return
    state.fetchLog.push({ url: String(url).slice(0, 160), at: Date.now() })
    captureUploadRefs(url, bodyText, responseText, taskToken)
    captureGenerate(url, bodyText, responseText, taskToken)
    captureHistory(url, bodyText, responseText, taskToken)
  }

  if (state.fetchHooked) return JSON.stringify({ hooked: true, reused: true })

  const origFetch = window.fetch
  const origFetchToString = Function.prototype.toString.call(origFetch)
  window.fetch = async function (...args) {
    const taskToken = state.task_token
    const url = toUrl(args[0])
    const bodyText = await readBody(args[0], args[1])
    const patchedBodyText = patchGenerateBodyText(url, bodyText)
    let callArgs = args
    if (patchedBodyText !== bodyText) {
      if (args[1] && typeof args[1] === 'object') {
        callArgs = [args[0], { ...args[1], body: patchedBodyText }]
      } else if (typeof args[0] === 'string') {
        callArgs = [
          args[0],
          {
            method: 'POST',
            body: patchedBodyText,
            headers: { 'content-type': 'application/json' },
          },
        ]
      } else if (args[0] && typeof Request !== 'undefined') {
        callArgs = [new Request(args[0], { body: patchedBodyText }), args[1]]
      }
    }
    noteGenerateRequest(url, taskToken)
    if (url.includes('/mweb/v1/') || url.includes('/commerce/')) {
      captureUploadRefs(url, patchedBodyText, '', taskToken)
      captureGenerate(url, patchedBodyText, '', taskToken)
    }
    const resp = await origFetch.apply(this, callArgs)
    if (url.includes('/mweb/v1/') || url.includes('/commerce/')) {
      try {
        const text = await resp.clone().text()
        processPair(url, patchedBodyText, text, taskToken)
      } catch (e) {
        state.lastInterceptorError = String(e?.message || e)
      }
    }
    return resp
  }
  disguiseFetch(window.fetch, origFetchToString)

  const origOpen = XMLHttpRequest.prototype.open
  const origSend = XMLHttpRequest.prototype.send
  XMLHttpRequest.prototype.open = function (method, url) {
    this.__mediagenJimengUrl = String(url || '')
    return origOpen.apply(this, arguments)
  }
  XMLHttpRequest.prototype.send = function (body) {
    const taskToken = state.task_token
    const url = this.__mediagenJimengUrl || ''
    const bodyText = typeof body === 'string' ? body : ''
    const patchedBodyText = patchGenerateBodyText(url, bodyText)
    const sendBody = patchedBodyText !== bodyText ? patchedBodyText : body
    noteGenerateRequest(url, taskToken)
    if (url.includes('/mweb/v1/') || url.includes('/commerce/')) {
      try {
        captureUploadRefs(url, patchedBodyText, '', taskToken)
        captureGenerate(url, patchedBodyText, '', taskToken)
      } catch (_) {}
    }
    this.addEventListener('load', () => {
      if (url.includes('/mweb/v1/') || url.includes('/commerce/')) {
        try {
          processPair(url, patchedBodyText, this.responseText || '', taskToken)
        } catch (_) {}
      }
    })
    return origSend.call(this, sendBody)
  }

  state.fetchHooked = true
  return JSON.stringify({ hooked: true, baseline: currentImgs.length })
})()

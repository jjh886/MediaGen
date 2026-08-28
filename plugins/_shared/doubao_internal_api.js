function doubaoInternalUuid() {
  return typeof crypto?.randomUUID === 'function'
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`
}

function doubaoInternalJson(value, fallback = {}) {
  if (value && typeof value === 'object') return value
  try {
    return JSON.parse(String(value || ''))
  } catch {
    return fallback
  }
}

function doubaoInternalCookie(name) {
  const prefix = `${name}=`
  const item = document.cookie
    .split(';')
    .map((value) => value.trim())
    .find((value) => value.startsWith(prefix))
  return item ? decodeURIComponent(item.slice(prefix.length)) : ''
}

function doubaoInternalIdentity(config) {
  const samantha = doubaoInternalJson(localStorage.getItem('samantha_web_web_id'))
  const tea = doubaoInternalJson(localStorage.getItem(`__tea_cache_tokens_${config.aid}`))
  const deviceId = String(samantha.web_id || tea.user_unique_id || tea.web_id || '')
  const webId = String(tea.web_id || deviceId || '')
  const fp = doubaoInternalCookie('s_v_web_id') || (deviceId ? `verify_${deviceId}` : '')
  return { deviceId, webId, fp }
}

function doubaoInternalQuery(config) {
  const identity = doubaoInternalIdentity(config)
  const params = {
    aid: config.aid,
    device_id: identity.deviceId,
    device_platform: 'web',
    fp: identity.fp,
    language: 'zh',
    pc_version: config.pcVersion,
    pkg_type: 'release_version',
    real_aid: config.aid,
    region: config.region || '',
    samantha_web: '1',
    sys_region: config.region || '',
    tea_uuid: identity.webId,
    'use-olympus-account': '1',
    version_code: '20800',
    web_id: identity.webId,
    web_tab_id: doubaoInternalUuid(),
    ...(config.query || {}),
  }
  return new URLSearchParams(
    Object.entries(params).filter(([, value]) => String(value || '').length > 0)
  ).toString()
}

async function doubaoInternalFetch(config, endpoint, payload, timeoutMs) {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    const csrf = doubaoInternalCookie('passport_csrf_token')
    const response = await fetch(
      `${doubaoInternalBaseUrl(config)}${endpoint}?${doubaoInternalQuery(config)}`,
      {
        method: 'POST',
        headers: {
          Accept: 'text/event-stream',
          'Content-Type': 'application/json',
          'agw-js-conv': 'str',
          ...(csrf ? { 'x-tt-passport-csrf-token': csrf } : {}),
        },
        credentials: 'include',
        body: JSON.stringify(payload),
        signal: controller.signal,
      }
    )
    const body = await response.text()
    if (!response.ok) throw new Error(`HTTP ${response.status}: ${body.slice(0, 500)}`)
    if (body.trim().startsWith('{')) {
      const error = doubaoInternalJson(body)
      if (error.code && error.code !== 0) {
        throw new Error(`Doubao ${error.code}: ${error.msg || error.message || '请求失败'}`)
      }
    }
    return body
  } finally {
    clearTimeout(timer)
  }
}

function doubaoInternalBaseUrl(config) {
  return String(config?.baseUrl || 'https://www.doubao.com').replace(/\/$/, '')
}

function doubaoInternalEvents(raw) {
  const events = []
  for (const block of String(raw || '')
    .replace(/\r\n/g, '\n')
    .split('\n\n')) {
    const data = block
      .split('\n')
      .map((line) => line.trim())
      .find((line) => line.startsWith('data:'))
    if (!data) continue
    const parsed = doubaoInternalJson(data.slice(5).trim(), null)
    if (parsed) events.push(parsed)
  }
  return events
}

function doubaoInternalEventData(event) {
  return doubaoInternalJson(event?.event_data, event?.event_data || {})
}

function doubaoInternalMessage(event) {
  return doubaoInternalJson(doubaoInternalEventData(event)?.message, {})
}

function doubaoInternalContent(message) {
  return doubaoInternalJson(message?.content, message?.content || {})
}

function doubaoInternalThrowEventError(events, label) {
  const failed = events.find((event) => Number(event?.event_type) === 2005)
  if (failed) {
    // 风控响应可能携带 decision.detail 等内部令牌，只向 API 暴露稳定错误码和消息。
    const detail = doubaoInternalEventData(failed)
    const code = detail?.code || detail?.error_detail?.code || 'platform_rejected'
    const message = detail?.message || detail?.error_detail?.message || '平台拒绝生成'
    throw new Error(`${label}: ${JSON.stringify({ code, message })}`)
  }
  const text = events
    .filter((event) => Number(event?.event_type) === 2001)
    .map(doubaoInternalMessage)
    .filter((message) => Number(message?.content_type) === 2001)
    .map((message) => String(doubaoInternalContent(message)?.text || ''))
    .join('')
  if (/服务过载|稍后重试|无法生成|内容违规|额度.*用完/.test(text)) throw new Error(text)
  return text
}

function doubaoInternalPayload(message, actionBarSkillId) {
  return {
    messages: [message],
    completion_option: {
      is_regen: false,
      with_suggest: true,
      need_create_conversation: true,
      launch_stage: 1,
      is_replace: false,
      is_delete: false,
      is_ai_playground: false,
      memory_type: 2,
      message_from: 0,
      use_deep_think: false,
      use_auto_cot: false,
      resend_for_regen: false,
      enable_commerce_credit: false,
      action_bar_skill_id: actionBarSkillId,
    },
    evaluate_option: { web_ab_params: '' },
    local_conversation_id: doubaoInternalUuid(),
    local_message_id: doubaoInternalUuid(),
  }
}

async function doubaoInternalUploadImage(config, dataUrl, index) {
  const match = String(dataUrl || '').match(/^data:(image\/[a-zA-Z0-9.+-]+);base64,(.+)$/s)
  if (!match) throw new Error(`第 ${index + 1} 张参考图不是有效 data URL`)
  const bytes = Uint8Array.from(atob(match[2]), (char) => char.charCodeAt(0))
  const extension = match[1].split('/')[1].replace('jpeg', 'jpg')
  const form = new FormData()
  form.append('data', new Blob([bytes], { type: match[1] }), `reference-${index}.${extension}`)
  form.append('file_type', extension)
  const upload = await fetch(
    `${doubaoInternalBaseUrl(config)}/samantha/pages/upload_image?${doubaoInternalQuery(config)}`,
    {
      method: 'POST',
      credentials: 'include',
      body: form,
    }
  )
  const uploadBody = await upload.json()
  if (!upload.ok || uploadBody.code !== 0 || !uploadBody.data?.uri) {
    throw new Error(`参考图上传失败: ${uploadBody.msg || upload.status}`)
  }
  const csrf = doubaoInternalCookie('passport_csrf_token')
  const fileResponse = await fetch(
    `${doubaoInternalBaseUrl(config)}/alice/message/get_file_url?${doubaoInternalQuery(config)}`,
    {
      method: 'POST',
      credentials: 'include',
      headers: {
        'Content-Type': 'application/json',
        ...(csrf ? { 'x-tt-passport-csrf-token': csrf } : {}),
      },
      body: JSON.stringify({
        uris: [uploadBody.data.uri],
        type: 'image',
        format: extension,
        expire_second: 3600,
      }),
    }
  )
  const fileBody = await fileResponse.json()
  const info = fileBody.data?.file_urls?.[0]
  if (!fileResponse.ok || fileBody.code !== 0 || !info) {
    throw new Error(`参考图地址获取失败: ${fileBody.msg || fileResponse.status}`)
  }
  return { key: info.uri || uploadBody.data.uri, url: info.main_url || '' }
}

async function doubaoInternalReferences(config, dataUrls, maxCount = 1) {
  const output = []
  for (const [index, dataUrl] of (dataUrls || []).slice(0, maxCount).entries()) {
    output.push(await doubaoInternalUploadImage(config, dataUrl, index))
  }
  return output
}

async function doubaoInternalGenerateImage(ctx, config) {
  const references = await doubaoInternalReferences(config, ctx.ref_image_dataurls, 1)
  const content = { text: ctx.prompt, ratio: ctx.image_aspect_ratio }
  const message = {
    content: JSON.stringify(content),
    content_type: 2009,
    attachments: references.map((item) => ({
      type: 'image',
      key: item.key,
      extra: { refer_types: 'overall' },
    })),
    references: [],
    skill: { skill_type: 3, skill_type_no_default: 3, skill_id: '3', skill_id_no_default: '3' },
  }
  const raw = await doubaoInternalFetch(
    config,
    '/samantha/chat/completion',
    doubaoInternalPayload(message, 3),
    180000
  )
  const events = doubaoInternalEvents(raw)
  doubaoInternalThrowEventError(events, '图片生成失败')
  const images = events
    .filter((event) => Number(event?.event_type) === 2001)
    .map(doubaoInternalMessage)
    .filter((item) => Number(item?.content_type) === 2010)
    .flatMap((item) => doubaoInternalContent(item)?.data || [])
    .map((item) => ({
      url: item.image_ori?.url || item.image_raw?.url || item.image_thumb?.url || '',
      width: item.image_ori?.width || item.image_thumb?.width || 0,
      height: item.image_ori?.height || item.image_thumb?.height || 0,
    }))
    .filter((item) => item.url)
  if (!images.length) throw new Error('图片生成完成，但平台没有返回图片')
  return { status: 'done', source: config.platformId, images }
}

async function doubaoInternalGenerateVideo(ctx, config) {
  const references = await doubaoInternalReferences(config, ctx.ref_image_dataurls, 1)
  const content = {
    text: ctx.prompt,
    ratio: ctx.video_aspect_ratio,
    ...(ctx.generation_options?.camera_movement
      ? { camera_movement: ctx.generation_options.camera_movement }
      : {}),
  }
  const message = {
    content: JSON.stringify(content),
    content_type: 2020,
    attachments: references.map((item) => ({ type: 'image', key: item.key })),
    references: [],
    skill: {
      skill_type: 17,
      skill_type_no_default: 17,
      skill_id: '17',
      skill_id_no_default: '17',
    },
  }
  const submitted = await doubaoInternalFetch(
    config,
    '/samantha/chat/completion',
    doubaoInternalPayload(message, 17),
    90000
  )
  const submitEvents = doubaoInternalEvents(submitted)
  const text = doubaoInternalThrowEventError(submitEvents, '视频生成失败')
  const taskId = submitEvents
    .filter((event) => Number(event?.event_type) === 2001)
    .map(doubaoInternalEventData)
    .find((data) => data?.fin_reason?.reason === 1)?.fin_reason?.async_task?.id
  if (!taskId) throw new Error(text || '视频生成没有返回 task_id')

  let completed
  try {
    completed = await doubaoInternalFetch(
      config,
      '/samantha/chat/async/stream',
      { task_id: taskId, event_id: 0 },
      600000
    )
  } catch {
    completed = await doubaoInternalFetch(
      config,
      '/samantha/chat/completion',
      { task_id: taskId, event_id: 0 },
      600000
    )
  }
  const events = doubaoInternalEvents(completed)
  doubaoInternalThrowEventError(events, '视频生成失败')
  const videos = events
    .filter((event) => Number(event?.event_type) === 2001)
    .map(doubaoInternalMessage)
    .filter((item) => Number(item?.content_type) === 2021)
    .flatMap((item) => {
      const contentValue = doubaoInternalContent(item)
      return Array.isArray(contentValue?.data) ? contentValue.data : [contentValue]
    })
    .map(doubaoInternalVideo)
    .filter((item) => item.video_url)
  if (!videos.length) throw new Error('视频任务结束，但平台没有返回成品地址')
  return { status: 'done', source: config.platformId, task_id: taskId, videos }
}

function doubaoInternalVideo(item) {
  let videoUrl = item?.video_url || item?.url || ''
  if (!videoUrl && item?.video_model) {
    const model = doubaoInternalJson(item.video_model)
    const encoded = Object.values(model.video_list || {}).find((value) => value?.main_url)?.main_url
    if (encoded) {
      try {
        videoUrl = atob(encoded)
      } catch {}
    }
  }
  return {
    video_url: videoUrl,
    cover_url: item?.cover_url || item?.cover?.url || '',
    width: item?.width || 0,
    height: item?.height || 0,
    duration: item?.duration || 0,
  }
}

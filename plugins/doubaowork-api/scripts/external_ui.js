const DOUBAOWORK_CDP_URL = 'http://[::1]:9222'
const DOUBAOWORK_EXECUTABLE = '/Applications/DoubaoWork.app/Contents/MacOS/DoubaoWork'
const DOUBAOWORK_UPLOAD_RUNTIME = `(async () => {
  const inputs = __MEDIAGEN_REFERENCE_INPUTS__
  const readJson = (value) => {
    try { return JSON.parse(value || '{}') } catch { return {} }
  }
  const cookie = (name) => {
    const prefix = name + '='
    const item = document.cookie.split(';').map(value => value.trim())
      .find(value => value.startsWith(prefix))
    return item ? decodeURIComponent(item.slice(prefix.length)) : ''
  }
  const query = () => {
    const samantha = readJson(localStorage.getItem('samantha_web_web_id'))
    const tea = readJson(localStorage.getItem('__tea_cache_tokens_1044603'))
    const deviceId = String(samantha.web_id || tea.user_unique_id || tea.web_id || '')
    const webId = String(tea.web_id || deviceId)
    return new URLSearchParams({
      aid: '1044603', real_aid: '1044603', device_id: deviceId,
      device_platform: 'web', doubao_device_platform: 'desktop',
      pc_version: '2.25.18', doubao_pc_version: '2.25.18',
      pkg_type: 'release_version', language: 'zh', region: 'CN', sys_region: 'CN',
      samantha_web: '1', tea_uuid: webId, web_id: webId, web_platform: 'desktop',
      'use-olympus-account': '1', version_code: '20800', runtime: 'web',
      runtime_version: '3.32.65', client_platform: 'pc_client',
      chromium_version: '147.0.7727.149', channel: 'mac_official',
      fp: cookie('s_v_web_id') || ('verify_' + deviceId), web_tab_id: crypto.randomUUID(),
    }).toString()
  }
  const output = []
  for (let index = 0; index < inputs.length; index += 1) {
    const match = String(inputs[index]).match(/^data:(image\\/[a-zA-Z0-9.+-]+);base64,(.+)$/s)
    if (!match) throw new Error('第 ' + (index + 1) + ' 张参考图格式无效')
    const bytes = Uint8Array.from(atob(match[2]), char => char.charCodeAt(0))
    const extension = match[1].split('/')[1].replace('jpeg', 'jpg')
    const form = new FormData()
    form.append('data', new Blob([bytes], { type: match[1] }), 'reference-' + index + '.' + extension)
    form.append('file_type', extension)
    const uploaded = await fetch(
      'https://www.doubao.com/samantha/pages/upload_image?' + query(),
      { method: 'POST', credentials: 'include', body: form }
    )
    const uploadBody = await uploaded.json()
    if (!uploaded.ok || uploadBody.code !== 0 || !uploadBody.data?.uri) {
      throw new Error('参考图上传失败: ' + (uploadBody.msg || uploaded.status))
    }
    const csrf = cookie('passport_csrf_token')
    const resolved = await fetch(
      'https://www.doubao.com/alice/message/get_file_url?' + query(),
      {
        method: 'POST', credentials: 'include',
        headers: {
          'Content-Type': 'application/json',
          ...(csrf ? { 'x-tt-passport-csrf-token': csrf } : {}),
        },
        body: JSON.stringify({
          uris: [uploadBody.data.uri], type: 'image', format: extension, expire_second: 3600,
        }),
      }
    )
    const resolvedBody = await resolved.json()
    const info = resolvedBody.data?.file_urls?.[0]
    if (!resolved.ok || resolvedBody.code !== 0 || !info?.main_url) {
      throw new Error('参考图地址获取失败: ' + (resolvedBody.msg || resolved.status))
    }
    output.push(info.main_url)
  }
  return output
})()`

function doubaoWorkExternalGenerate(ctx, kind) {
  doubaoWorkEnsureDebugApp()
  const target = doubaoWorkCreateChatTarget()
  const targetId = String(target.id || '')
  if (!targetId) throw new Error('Doubao Work 没有返回新聊天页 target_id')
  host.call('external_cdp.register_target', {
    debug_http_url: DOUBAOWORK_CDP_URL,
    task_id: ctx.task_id,
    target_id: targetId,
  })
  let result
  try {
    doubaoWorkWaitForEditor(targetId)
    const before = doubaoWorkUiSnapshot(targetId)
    const referenceUrls = doubaoWorkUploadReferences(ctx.ref_image_dataurls || [])
    doubaoWorkSubmit(targetId, doubaoWorkInstruction(ctx, kind, referenceUrls))
    result = doubaoWorkWaitForResult(targetId, before, kind)
  } catch (error) {
    doubaoWorkCleanupTarget(ctx.task_id, targetId)
    throw error
  }
  doubaoWorkCleanupTarget(ctx.task_id, targetId)
  return result
}

function doubaoWorkCleanupTarget(taskId, targetId) {
  host.tryCall('external_cdp.close_target', {
    debug_http_url: DOUBAOWORK_CDP_URL,
    target_id: targetId,
  })
  host.tryCall('external_cdp.unregister_target', {
    task_id: taskId,
    target_id: targetId,
  })
}

function doubaoWorkEnsureDebugApp() {
  const listed = host.tryCall('external_cdp.list_targets', {
    debug_http_url: DOUBAOWORK_CDP_URL,
  })
  if (listed.success) return listed.data
  host.call('process.spawn', {
    command: DOUBAOWORK_EXECUTABLE,
    args: ['--remote-debugging-port=9222', '--remote-allow-origins=*'],
  })
  for (let attempt = 0; attempt < 16; attempt += 1) {
    host.sleep(500)
    const retry = host.tryCall('external_cdp.list_targets', {
      debug_http_url: DOUBAOWORK_CDP_URL,
    })
    if (retry.success) return retry.data
  }
  throw new Error(
    'Doubao Work 未开启 CDP。请关闭正在运行的普通实例后重试，MediaGen 会以调试模式启动它。'
  )
}

function doubaoWorkCreateChatTarget() {
  const created = host.tryCall('external_cdp.create_target', {
    debug_http_url: DOUBAOWORK_CDP_URL,
    url: 'doubaowork://doubaowork-chat/chat',
  })
  if (created.success && created.data?.id) return created.data
  const targets = host.call('external_cdp.list_targets', {
    debug_http_url: DOUBAOWORK_CDP_URL,
  })
  const existing = targets.find(
    (target) => target.type === 'page' && /doubaowork-(chat|launcher)\//.test(target.url || '')
  )
  if (!existing) throw new Error(created.error || 'Doubao Work 没有可用聊天页面')
  return existing
}

function doubaoWorkWaitForEditor(targetId) {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    const ready = doubaoWorkEvaluate(
      targetId,
      // 空白聊天页只有输入框；发送按钮要在写入文字后才会挂载。
      `Boolean(document.querySelector('[contenteditable="true"]'))`
    )
    if (ready) return
    host.sleep(500)
  }
  throw new Error('Doubao Work 新聊天页在 30 秒内没有准备好')
}

function doubaoWorkUploadReferences(dataUrls) {
  if (!Array.isArray(dataUrls) || !dataUrls.length) return []
  const target = host.call('external_cdp.create_target', {
    debug_http_url: DOUBAOWORK_CDP_URL,
    url: 'https://www.doubao.com/chat/',
  })
  const targetId = String(target.id || '')
  if (!targetId) throw new Error('Doubao Work 参考图上传页启动失败')
  for (let attempt = 0; attempt < 60; attempt += 1) {
    host.sleep(500)
    const ready = doubaoWorkEvaluate(
      targetId,
      `location.origin === 'https://www.doubao.com' && Boolean(localStorage.getItem('samantha_web_web_id'))`
    )
    if (ready) break
  }
  const urls = host.call('external_cdp.evaluate', {
    debug_http_url: DOUBAOWORK_CDP_URL,
    target_id: targetId,
    expression: doubaoWorkUploadExpression(dataUrls),
    await_promise: true,
    timeout_ms: 180000,
  })
  host.tryCall('external_cdp.close_target', {
    debug_http_url: DOUBAOWORK_CDP_URL,
    target_id: targetId,
  })
  return urls
}

function doubaoWorkUploadExpression(dataUrls) {
  return DOUBAOWORK_UPLOAD_RUNTIME.replace(
    '__MEDIAGEN_REFERENCE_INPUTS__',
    JSON.stringify(dataUrls)
  )
}

function doubaoWorkInstruction(ctx, kind, referenceUrls) {
  const referenceText = referenceUrls.length
    ? `\n参考素材 URL（必须作为独立参考输入，不得写入 prompt）：\n${referenceUrls
        .map((url, index) => `${index + 1}. ${url}`)
        .join('\n')}`
    : ''
  if (kind === 'video') {
    return `[Seedance 2.5](skill://seedance-25?type=2&id=353376777986) 请生成视频。原始 prompt 必须逐字透传：\n${ctx.prompt}\n时长 ${ctx.seconds} 秒，比例 ${ctx.video_aspect_ratio}。以上参数已经确认，请立即使用 Seedance 2.5 生成。${referenceText}`
  }
  return `[Seedream 5.0 Pro](skill://seedream-50?type=2&id=360075272194) 请使用 Seedream 5.0 Pro 直接生成图片。原始需求：\n${ctx.prompt}\n比例 ${ctx.image_aspect_ratio}，不要切换模型。${referenceText}`
}

function doubaoWorkSubmit(targetId, text) {
  const ready = doubaoWorkEvaluate(
    targetId,
    `(() => {
      const editor = document.querySelector('[contenteditable="true"]')
      if (!editor) return false
      editor.focus()
      document.execCommand('selectAll', false)
      document.execCommand('delete', false)
      return true
    })()`
  )
  if (!ready) throw new Error('Doubao Work 输入框不可用')
  doubaoWorkCommand(targetId, 'Input.insertText', { text })
  host.sleep(500)
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const submitted = doubaoWorkEvaluate(
      targetId,
      `(() => {
        const button = document.querySelector('#flow-end-msg-send')
        if (!button || button.disabled) return false
        button.click()
        return true
      })()`
    )
    if (submitted) return
    host.sleep(250)
  }
  throw new Error('Doubao Work 发送按钮不可用')
}

function doubaoWorkWaitForResult(targetId, before, kind) {
  let confirmationSent = false
  const attempts = kind === 'video' ? 180 : 120
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    host.sleep(5000)
    const snapshot = doubaoWorkUiSnapshot(targetId)
    const error = doubaoWorkResultError(snapshot.text, kind)
    if (error) throw new Error(error)
    if (doubaoWorkNeedsConfirmation(snapshot.text, kind, confirmationSent)) {
      doubaoWorkSubmit(targetId, '确认，按以上信息使用 Seedance 2.5 生成。')
      confirmationSent = true
    }
    const result = doubaoWorkMediaResult(snapshot, before, kind)
    if (result) return result
  }
  throw new Error(`Doubao Work ${kind === 'video' ? '视频' : '图片'}生成超时`)
}

function doubaoWorkResultError(text, kind) {
  const sharedRules = [
    [/710022004|rate limited|需要验证/, '710022004: Doubao Work 需要完成风控验证'],
    [/我暂时无法生成|未认证人脸暂不支持/, 'Doubao Work 拒绝了本次媒体生成'],
  ]
  const videoRule = [
    /Seedance 2\.5 (?:需开通加强版或高级版会员|为会员专属模型)|当前账号暂未开通加强版本?会员或高级版本?会员|当前账号暂不支持|可以改用 Seedance 2\.0 Fast|seedance_2\.0_fast/,
    '当前 Doubao Work 账号没有 Seedance 2.5 权限，已拒绝模型降级',
  ]
  const imageRule = [
    /Seedream 5\.0 Pro 是会员专属模型|当前账号暂未开通会员|已用 Seedream 4\.5 生成|改用 Seedream 4\.5/,
    '当前 Doubao Work 账号没有 Seedream 5.0 Pro 权限，已拒绝模型降级',
  ]
  const modelRule = kind === 'video' ? videoRule : imageRule
  return [...sharedRules, modelRule].find(([pattern]) => pattern.test(text))?.[1] || ''
}

function doubaoWorkNeedsConfirmation(text, kind, confirmationSent) {
  return (
    kind === 'video' && !confirmationSent && /是否按以上信息|是否使用 Seedance 2\.5 生成/.test(text)
  )
}

function doubaoWorkMediaResult(snapshot, before, kind) {
  const images = snapshot.images.filter((url) => !before.images.includes(url))
  const videos = snapshot.videos.filter((url) => !before.videos.includes(url))
  const urls = kind === 'image' ? images : videos
  if (!urls.length) return null
  return kind === 'image'
    ? { status: 'done', source: 'doubaowork-api', images: urls.map((url) => ({ url })) }
    : {
        status: 'done',
        source: 'doubaowork-api',
        videos: urls.map((video_url) => ({ video_url })),
      }
}

function doubaoWorkUiSnapshot(targetId) {
  return doubaoWorkEvaluate(
    targetId,
    `(() => ({
      text: (document.body.innerText || '').slice(-12000),
      images: [...document.images]
        .map(image => image.currentSrc || image.src || '')
        .filter(url => url.includes('/rc_gen_image/')),
      videos: [...new Set([
        ...[...document.querySelectorAll('video, video source')].map(item => item.currentSrc || item.src || ''),
        ...performance.getEntriesByType('resource').map(item => item.name || ''),
      ])].filter(url => /^https?:/.test(url) && (
        url.includes('vdl.doubao.com') ||
        url.includes('douyinvod.com') ||
        url.includes('/video/tos/')
      )),
    }))()`
  )
}

function doubaoWorkEvaluate(targetId, expression) {
  return host.call('external_cdp.evaluate', {
    debug_http_url: DOUBAOWORK_CDP_URL,
    target_id: targetId,
    expression,
    await_promise: false,
    timeout_ms: 30000,
  })
}

function doubaoWorkCommand(targetId, method, params) {
  return host.call('external_cdp.command', {
    debug_http_url: DOUBAOWORK_CDP_URL,
    target_id: targetId,
    method,
    params,
    timeout_ms: 30000,
  })
}

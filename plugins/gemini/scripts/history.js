function geminiHistory(session, saved) {
  if (!/^c_[a-zA-Z0-9_-]+$/.test(saved.conversation_id || ''))
    throw new Error('invalid_response: Gemini 会话编号无效')
  const value = geminiRpc(session, 'hNvQHb', [
    saved.conversation_id,
    10,
    null,
    1,
    [0],
    [4],
    null,
    1,
  ])
  const turn = value?.[0]?.find((turn) => turn?.[0]?.[1] === saved.response_id)
  if (!turn) return null
  const candidate = turn[3]?.[0]?.[0]
  return {
    turn,
    text: typeof candidate?.[1]?.[0] === 'string' ? candidate[1][0] : '',
    candidate_id: candidate?.[0] || saved.candidate_id,
  }
}

function geminiWalk(value, visit, depth = 0) {
  if (depth > 40) return
  visit(value)
  if (Array.isArray(value)) for (const item of value) geminiWalk(item, visit, depth + 1)
}

function geminiMediaURLs(value, kind) {
  const urls = new Set()
  geminiWalk(value, (item) => {
    if (typeof item !== 'string') return
    if (kind === 'image' && /^https:\/\/lh3\.googleusercontent\.com\/gg(?:-dl)?\//.test(item))
      urls.add(item)
    if (
      kind === 'video' &&
      /^https:\/\/contribution\.usercontent\.google\.com\/download\?/.test(item)
    ) {
      const encoded = /[?&]c=([^&]+)/.exec(item)?.[1]
      if (!encoded) return
      try {
        const text = decodeURIComponent(encoded).replace(/-/g, '+').replace(/_/g, '/')
        const decoded = host.call('crypto.base64_decode_latin1', {
          text: text.padEnd(Math.ceil(text.length / 4) * 4, '='),
        }).value
        if (decoded.includes('response_data')) urls.add(item)
      } catch (_) {}
    }
  })
  return [...urls]
}

function geminiMediaFailure(history, kind) {
  const raw = JSON.stringify(history.turn?.[3] || [])
  if (kind === 'video' && /out of video generation quota|as soon as your limit resets/.test(raw)) {
    const reset = Math.max(
      0,
      ...[...raw.matchAll(/\[(1[7-9]\d{8}),\d+\]/g)]
        .map((match) => Number(match[1]))
        .filter((time) => time > Date.now() / 1000)
    )
    throw new Error(
      'quota_exhausted: Gemini 视频额度耗尽' +
        (reset
          ? '; retry after ' + Math.ceil(reset - Date.now() / 1000) + ' seconds'
          : '；上游未提供恢复时间')
    )
  }
  if (/can't generate that video|cannot generate that video/i.test(history.text))
    throw new Error('content_policy_violation: Gemini 拒绝此视频请求')
  if (kind === 'video' && history.text && !raw.includes('video_generation'))
    throw new Error('media_generation_refused: Gemini 未启动视频生成；' + history.text)
}

function geminiDownload(session, original, kind) {
  let url = original
  if (kind === 'image') url = original.replace(/=[^/]*$/, '') + '=s0'
  else if (!/[?&]opi=/.test(url)) url += '&filename=video.mp4&opi=103135050'
  for (let hop = 0; hop < 5; hop++) {
    const hostname = /^https:\/\/([^/?#]+)/.exec(url)?.[1]
    if (
      !hostname ||
      ![
        'lh3.googleusercontent.com',
        'lh3.google.com',
        'work.fife.usercontent.google.com',
        'contribution.usercontent.google.com',
      ].includes(hostname)
    )
      throw new Error('download_failed: Gemini 媒体下载域名无效')
    // 下载端仅携带 Google 域作用域的 Cookie，不能转发主机专属凭据。
    const allowedCookies = [
      'HSID',
      'SSID',
      'APISID',
      'SAPISID',
      '__Secure-1PAPISID',
      '__Secure-3PAPISID',
      'SID',
      '__Secure-1PSID',
      '__Secure-3PSID',
      'GOOGLE_ABUSE_EXEMPTION',
      'NID',
      '__Secure-1PSIDTS',
      '__Secure-1PSIDRTS',
      '__Secure-3PSIDTS',
      '__Secure-3PSIDRTS',
      'SIDCC',
      '__Secure-1PSIDCC',
      '__Secure-3PSIDCC',
    ]
    const cookie = session.cookies
      .filter(
        (c) =>
          allowedCookies.includes(c.name) &&
          String(c.domain).replace(/^\./, '') === 'google.com' &&
          !c.hostOnly &&
          (!c.expires || c.expires <= 0 || c.expires > Date.now() / 1000)
      )
      .map((c) => c.name + '=' + c.value)
      .join('; ')
    const response = host.call('http.request', {
      url,
      method: 'GET',
      proxy: 'system',
      follow_redirects: false,
      response_body_format: 'base64',
      timeout_ms: 120000,
      retry: { max_attempts: 1 },
      headers: { cookie, referer: GEMINI_ORIGIN + '/', accept: '*/*' },
    })
    if ([301, 302, 303, 307, 308].includes(Number(response.status))) {
      url = response.headers?.location || ''
      continue
    }
    if (!response.ok || !response.body_base64)
      throw new Error('download_failed: Gemini 产物下载 HTTP ' + response.status)
    const mime = String(response.headers?.['content-type'] || '').split(';')[0]
    if (kind === 'video' && mime === 'application/protobuf') return ''
    if (!mime.startsWith(kind + '/')) throw new Error('download_failed: Gemini 产物内容类型不符')
    if (kind === 'video') {
      const header = host.call('crypto.base64_decode_latin1', {
        text: response.body_base64.slice(0, 48),
      }).value
      if (header.slice(4, 8) !== 'ftyp') throw new Error('download_failed: Gemini 视频不是有效 MP4')
    }
    return 'data:' + mime + ';base64,' + response.body_base64
  }
  throw new Error('download_failed: Gemini 媒体重定向次数过多')
}

function geminiWaitMedia(session, saved, kind) {
  const deadline = Date.now() + 20 * 60 * 1000
  while (Date.now() < deadline) {
    const history = geminiHistory(session, saved)
    if (history) {
      const urls = geminiMediaURLs(history.turn[3], kind)
      if (urls.length) {
        if (kind === 'image') return urls.map((url) => geminiDownload(session, url, kind))
        // 同一视频还附带 protobuf 元数据下载链接，只返回实际 MP4。
        for (const url of urls) {
          const video = geminiDownload(session, url, kind)
          if (video) return [video]
        }
        throw new Error('download_failed: Gemini 视频链接未包含 MP4 产物')
      }
      geminiMediaFailure(history, kind)
    }
    host.call('time.sleep', { milliseconds: 5000 })
  }
  throw new Error('temporary_unavailable: Gemini 媒体仍未完成，请恢复原任务；不会重复提交')
}

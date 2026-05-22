// VolcEngine ImageX 参考图上传工具（豆包等字节系平台通用）
// ------------------------------------------------------------------
// 暴露全局函数 window.__volcImagexUpload(opts)。完整 4 步：
//   1. prepareSts        POST /alice/resource/prepare_upload（Cookie 认证）
//   2. applyImageUpload  GET  /top/v1?Action=ApplyImageUpload（AWS V4 签名）
//   3. putBlob           POST <UploadHost>/upload/v1/<StoreUri>（Content-CRC32 + X-Storage-U）
//   4. commitImageUpload POST /top/v1?Action=CommitImageUpload（AWS V4 签名 + SessionKey）
//
// opts:
//   blob      Blob         必填，图片原始字节
//   userId    string       必填，X-Storage-U 用（登录后站点的 user_id）
//   serviceId string       必填，豆包用 'a9rns2rl98'
//   region?   string       默认 'cn-north-1'
//   service?  string       默认 'imagex'
//   prepareUploadUrl? string 默认 https://www.doubao.com/alice/resource/prepare_upload
//   topApiUrl?        string 默认 https://www.doubao.com/top/v1
//
// 返回: { uri, width, height }
;(() => {
  if (typeof window === 'undefined' || window.__volcImagexUpload) return

  // ── CRC-32 (IEEE 802.3) ─────────────────────────────────────────
  const CRC32_TABLE = (() => {
    const t = new Uint32Array(256)
    for (let i = 0; i < 256; i++) {
      let c = i
      for (let j = 0; j < 8; j++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
      t[i] = c >>> 0
    }
    return t
  })()
  function crc32Hex(bytes) {
    let c = 0xffffffff
    for (let i = 0; i < bytes.length; i++) c = CRC32_TABLE[(c ^ bytes[i]) & 0xff] ^ (c >>> 8)
    return ((c ^ 0xffffffff) >>> 0).toString(16).padStart(8, '0')
  }

  // ── 小工具 ───────────────────────────────────────────────────────
  function u8ToHex(u8) {
    let s = ''
    for (let i = 0; i < u8.length; i++) s += u8[i].toString(16).padStart(2, '0')
    return s
  }
  async function sha256Hex(data) {
    const buf = typeof data === 'string' ? new TextEncoder().encode(data) : data
    const hash = await crypto.subtle.digest('SHA-256', buf)
    return u8ToHex(new Uint8Array(hash))
  }
  async function hmacSha256(key, data) {
    const k = typeof key === 'string' ? new TextEncoder().encode(key) : key
    const d = typeof data === 'string' ? new TextEncoder().encode(data) : data
    const ck = await crypto.subtle.importKey('raw', k, { name: 'HMAC', hash: 'SHA-256' }, false, [
      'sign',
    ])
    return new Uint8Array(await crypto.subtle.sign('HMAC', ck, d))
  }
  function awsNow() {
    // 20260413T121308Z
    return new Date()
      .toISOString()
      .replace(/[:-]/g, '')
      .replace(/\.\d{3}/, '')
  }
  function randS(n = 11) {
    const chars = 'abcdefghijklmnopqrstuvwxyz0123456789'
    let s = ''
    for (let i = 0; i < n; i++) s += chars[Math.floor(Math.random() * chars.length)]
    return s
  }
  function extFromBlob(blob) {
    const m = {
      'image/png': '.png',
      'image/jpeg': '.jpg',
      'image/jpg': '.jpg',
      'image/webp': '.webp',
      'image/gif': '.gif',
    }
    return m[blob.type] || '.png'
  }
  function getImageDimensions(blob) {
    return new Promise((resolve) => {
      const url = URL.createObjectURL(blob)
      const img = new Image()
      const cleanup = () => URL.revokeObjectURL(url)
      img.onload = () => {
        resolve({ width: img.naturalWidth, height: img.naturalHeight })
        cleanup()
      }
      img.onerror = () => {
        resolve({ width: 0, height: 0 })
        cleanup()
      }
      img.src = url
    })
  }

  // ── AWS V4 签名（浏览器禁止设 host header，但签名照签，浏览器自动补的 Host 与签名一致即可） ──
  function sortByByte(a, b) {
    return a < b ? -1 : a > b ? 1 : 0
  }

  function buildCanonicalQuery(urlObj) {
    const pairs = [...urlObj.searchParams.entries()].sort(([a], [b]) => sortByByte(a, b))
    return pairs.map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join('&')
  }

  function buildCanonicalHeaders(headers) {
    const entries = Object.entries(headers).map(([k, v]) => [k.toLowerCase(), String(v).trim()])
    entries.sort(([a], [b]) => sortByByte(a, b))
    return {
      canonical: entries.map(([k, v]) => `${k}:${v}\n`).join(''),
      signed: entries.map(([k]) => k).join(';'),
    }
  }

  async function awsV4Signature({
    method,
    url,
    signedHeaderMap,
    bodyHash,
    accessKey,
    secretKey,
    region,
    service,
    now,
  }) {
    const u = new URL(url)
    const canonicalQuery = buildCanonicalQuery(u)
    const { canonical: canonicalHeaders, signed: signedHeaders } =
      buildCanonicalHeaders(signedHeaderMap)
    const canonicalRequest = [
      method.toUpperCase(),
      u.pathname,
      canonicalQuery,
      canonicalHeaders,
      signedHeaders,
      bodyHash,
    ].join('\n')

    const ymd = now.slice(0, 8)
    const credentialScope = `${ymd}/${region}/${service}/aws4_request`
    const stringToSign = [
      'AWS4-HMAC-SHA256',
      now,
      credentialScope,
      await sha256Hex(canonicalRequest),
    ].join('\n')

    const kDate = await hmacSha256('AWS4' + secretKey, ymd)
    const kRegion = await hmacSha256(kDate, region)
    const kService = await hmacSha256(kRegion, service)
    const kSigning = await hmacSha256(kService, 'aws4_request')
    const signature = u8ToHex(await hmacSha256(kSigning, stringToSign))

    return {
      authorization: `AWS4-HMAC-SHA256 Credential=${accessKey}/${credentialScope}, SignedHeaders=${signedHeaders}, Signature=${signature}`,
      signedHeaders,
    }
  }

  // ── 步骤 1: prepare_upload ──────────────────────────────────────
  async function prepareSts(prepareUrl) {
    const resp = await fetch(prepareUrl, {
      method: 'POST',
      credentials: 'include',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ tenant_id: '5', scene_id: '5', resource_type: 2 }),
    })
    if (!resp.ok) throw new Error(`prepare_upload HTTP ${resp.status}`)
    const data = await resp.json()
    if (data.code !== 0) throw new Error(`prepare_upload code=${data.code} ${data.msg || ''}`)
    const auth = data.data?.upload_auth_token
    if (!auth?.access_key) throw new Error('prepare_upload 缺少 upload_auth_token')
    return {
      accessKey: auth.access_key,
      secretKey: auth.secret_key,
      sessionToken: auth.session_token,
    }
  }

  // ── 步骤 2: ApplyImageUpload（AWS V4 签名） ──────────────────────
  async function applyImageUpload({
    topApiUrl,
    serviceId,
    fileSize,
    fileExt,
    sts,
    region,
    service,
  }) {
    const qs = new URLSearchParams({
      Action: 'ApplyImageUpload',
      Version: '2018-08-01',
      ServiceId: serviceId,
      NeedFallback: 'true',
      FileSize: String(fileSize),
      FileExtension: fileExt,
      s: randS(11),
    })
    const url = `${topApiUrl}?${qs.toString()}`
    const u = new URL(url)
    const now = awsNow()
    const signedHeaderMap = {
      host: u.host,
      'x-amz-date': now,
      'x-amz-security-token': sts.sessionToken,
    }
    const emptyHash = await sha256Hex('')
    const { authorization } = await awsV4Signature({
      method: 'GET',
      url,
      signedHeaderMap,
      bodyHash: emptyHash,
      accessKey: sts.accessKey,
      secretKey: sts.secretKey,
      region,
      service,
      now,
    })
    const resp = await fetch(url, {
      method: 'GET',
      credentials: 'include',
      headers: {
        'X-Amz-Date': now,
        'X-Amz-Security-Token': sts.sessionToken,
        Authorization: authorization,
      },
    })
    if (!resp.ok) throw new Error(`ApplyImageUpload HTTP ${resp.status}`)
    const data = await resp.json()
    const addr = data.Result?.UploadAddress
    const store = addr?.StoreInfos?.[0]
    if (!store?.StoreUri)
      throw new Error('ApplyImageUpload 响应缺少 StoreUri: ' + JSON.stringify(data))
    return {
      storeUri: store.StoreUri,
      uploadId: store.UploadID,
      uploadHost: addr.UploadHosts?.[0] || 'tos-lf-x.snssdk.com',
      sessionKey: addr.SessionKey,
    }
  }

  // ── 步骤 3: PUT/POST 字节到 TOS ─────────────────────────────────
  // 走本地 Rust 代理（/api/internal/imagex_put_blob），绕开浏览器对
  // tos-*.snssdk.com 跨域 POST 的 CORS 拒绝（不同区域节点 CORS 策略不一）。
  async function putBlob({ uploadHost, storeUri, crc32, userId, blob }) {
    const params = new URLSearchParams({
      host: uploadHost,
      uri: storeUri,
      crc32,
      user_id: String(userId || ''),
    })
    const url = `http://127.0.0.1:18080/api/internal/imagex_put_blob?${params.toString()}`
    const resp = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/octet-stream' },
      body: blob,
    })
    if (!resp.ok) {
      const txt = await resp.text().catch(() => '')
      throw new Error(`upload HTTP ${resp.status}: ${txt.slice(0, 200)}`)
    }
    // 2xx 即视作成功。部分 TOS 节点返回 204 No Content（空 body），其他节点返回 200+JSON。
    // 空 body 直接放行；有 body 则尝试解析 code，仅在 code 存在且异常时才失败。
    const txt = await resp.text().catch(() => '')
    if (!txt) return
    try {
      const data = JSON.parse(txt)
      if (data.code != null && data.code !== 2000 && data.code !== 0) {
        throw new Error('upload 响应异常: ' + txt.slice(0, 200))
      }
    } catch (e) {
      // 响应不是 JSON 但 HTTP 2xx，视为成功
    }
  }

  // ── 步骤 4: CommitImageUpload（AWS V4 签名 + Body） ──────────────
  async function commitImageUpload({ topApiUrl, serviceId, sessionKey, sts, region, service }) {
    const qs = new URLSearchParams({
      Action: 'CommitImageUpload',
      Version: '2018-08-01',
      ServiceId: serviceId,
    })
    const url = `${topApiUrl}?${qs.toString()}`
    const u = new URL(url)
    const now = awsNow()
    const bodyStr = JSON.stringify({ SessionKey: sessionKey })
    const bodyHash = await sha256Hex(bodyStr)
    const signedHeaderMap = {
      host: u.host,
      'x-amz-date': now,
      'x-amz-security-token': sts.sessionToken,
      'content-type': 'application/json',
    }
    const { authorization } = await awsV4Signature({
      method: 'POST',
      url,
      signedHeaderMap,
      bodyHash,
      accessKey: sts.accessKey,
      secretKey: sts.secretKey,
      region,
      service,
      now,
    })
    const resp = await fetch(url, {
      method: 'POST',
      credentials: 'include',
      headers: {
        'Content-Type': 'application/json',
        'X-Amz-Date': now,
        'X-Amz-Security-Token': sts.sessionToken,
        Authorization: authorization,
      },
      body: bodyStr,
    })
    if (!resp.ok) throw new Error(`CommitImageUpload HTTP ${resp.status}`)
    const data = await resp.json()
    const uri = data.Result?.Results?.[0]?.Uri
    if (!uri) throw new Error('CommitImageUpload 响应缺少 Uri: ' + JSON.stringify(data))
    // PluginResult 含服务端识别的权威 width/height/format，比浏览器侧 Image() 读更可靠
    const plugin = data.Result?.PluginResult?.[0] || {}
    return {
      uri,
      width: plugin.ImageWidth || 0,
      height: plugin.ImageHeight || 0,
      format: plugin.ImageFormat || '',
    }
  }

  // ── 顶层封装 ─────────────────────────────────────────────────────
  // 失败时把当前 phase 塞进 error.message，方便上层定位是哪一步。
  window.__volcImagexUpload = async function uploadOne(opts) {
    const {
      blob,
      userId,
      serviceId,
      region = 'cn-north-1',
      service = 'imagex',
      prepareUploadUrl = 'https://www.doubao.com/alice/resource/prepare_upload',
      topApiUrl = 'https://www.doubao.com/top/v1',
      sts: stsOverride,
    } = opts

    const fileSize = blob.size
    const fileExt = extFromBlob(blob)
    const bytes = new Uint8Array(await blob.arrayBuffer())
    const crc32 = crc32Hex(bytes)
    const { width, height } = await getImageDimensions(blob)

    let phase = 'prepare_sts'
    try {
      const sts = stsOverride || (await prepareSts(prepareUploadUrl))
      phase = 'apply_image_upload'
      const applied = await applyImageUpload({
        topApiUrl,
        serviceId,
        fileSize,
        fileExt,
        sts,
        region,
        service,
      })
      phase = `put_blob[host=${applied.uploadHost},uri=${applied.storeUri.slice(0, 30)}]`
      await putBlob({
        uploadHost: applied.uploadHost,
        storeUri: applied.storeUri,
        crc32,
        userId,
        blob,
      })
      phase = 'commit_image_upload'
      const committed = await commitImageUpload({
        topApiUrl,
        serviceId,
        sessionKey: applied.sessionKey,
        sts,
        region,
        service,
      })
      // 优先用服务端返回的权威尺寸；Fallback 才用浏览器 Image 读出来的
      return {
        uri: committed.uri,
        width: committed.width || width,
        height: committed.height || height,
        format: committed.format || '',
      }
    } catch (e) {
      const msg = String(e?.message || e)
      const err = new Error(`[${phase}] ${msg}`)
      err.phase = phase
      err.original = msg
      throw err
    }
  }

  // 为批量上传复用同一组 STS 凭证，减少 prepare_upload 次数
  window.__volcImagexUploadBatch = async function uploadBatch(blobs, opts) {
    const {
      userId,
      serviceId,
      region = 'cn-north-1',
      service = 'imagex',
      prepareUploadUrl = 'https://www.doubao.com/alice/resource/prepare_upload',
      topApiUrl = 'https://www.doubao.com/top/v1',
    } = opts
    const sts = await prepareSts(prepareUploadUrl)
    const results = []
    for (const blob of blobs) {
      const r = await window.__volcImagexUpload({
        blob,
        userId,
        serviceId,
        region,
        service,
        prepareUploadUrl,
        topApiUrl,
        sts,
      })
      results.push(r)
    }
    return results
  }
})()

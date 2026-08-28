#!/usr/bin/env node

import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'

const DEBUG_HOST = process.env.DOUBAOWORK_CDP_HOST || '::1'
const DEBUG_PORT = Number(process.env.DOUBAOWORK_CDP_PORT || 9222)
const MAX_TEXT_BODY_BYTES = 2 * 1024 * 1024
const outputPath = process.argv[2] || defaultOutputPath()

// 认证信息只用于当前 Doubao Work 会话，抓包证据里必须统一脱敏。
const secretKeyPattern =
  /^(authorization|proxy-authorization|cookie|set-cookie|x-tt-passport-csrf-token|x-csrf-token|csrf_token|passport_csrf_token|sessionid|session_id|sid|msToken|a_bogus|access_token|refresh_token|id_token|client_secret|signature)$/i

fs.mkdirSync(path.dirname(outputPath), { recursive: true })

const browserInfo = await getJson('/json/version')
const websocket = new WebSocket(browserInfo.webSocketDebuggerUrl)
const pendingCommands = new Map()
const sessions = new Map()
const requests = new Map()
let commandId = 0

websocket.addEventListener('open', async () => {
  try {
    await send('Target.setDiscoverTargets', { discover: true })
    await send('Target.setAutoAttach', {
      autoAttach: true,
      waitForDebuggerOnStart: false,
      flatten: true,
    })

    // setAutoAttach 对已有页面的行为随 Chromium 版本变化，因此显式补挂一次。
    const { targetInfos = [] } = await send('Target.getTargets')
    for (const target of targetInfos) {
      if (shouldCaptureTarget(target) && !hasTarget(target.targetId)) {
        await send('Target.attachToTarget', { targetId: target.targetId, flatten: true })
      }
    }

    writeEvent('capture_started', {
      browser: browserInfo.Browser,
      protocol_version: browserInfo['Protocol-Version'],
      output_path: outputPath,
    })
    process.stdout.write(`CAPTURE_READY ${outputPath}\n`)
  } catch (error) {
    fail(error)
  }
})

websocket.addEventListener('message', async (event) => {
  const message = JSON.parse(String(event.data))
  if (message.id) {
    settleCommand(message)
    return
  }

  try {
    await handleEvent(message)
  } catch (error) {
    writeEvent('capture_error', { message: errorMessage(error), method: message.method })
  }
})

websocket.addEventListener('error', (event) => fail(event.error || new Error('CDP 连接失败')))
websocket.addEventListener('close', () => {
  writeEvent('capture_stopped', {})
  process.stdout.write(`CAPTURE_STOPPED ${outputPath}\n`)
  process.exit(0)
})

process.on('SIGINT', () => websocket.close())
process.on('SIGTERM', () => websocket.close())

async function handleEvent(message) {
  const { method, params = {}, sessionId = '' } = message
  if (method === 'Target.attachedToTarget') {
    await enableTarget(params.sessionId, params.targetInfo)
    return
  }
  if (method === 'Target.detachedFromTarget') {
    sessions.delete(params.sessionId)
    return
  }

  const target = sessions.get(sessionId)
  if (!target) return

  if (method === 'Network.requestWillBeSent') {
    const key = requestKey(sessionId, params.requestId)
    const entry = {
      request_id: params.requestId,
      target,
      type: params.type,
      document_url: redactUrl(params.documentURL),
      request: redactRequest(params.request),
      initiator: redactValue(params.initiator),
    }
    requests.set(key, { ...entry, rawUrl: params.request?.url || '' })
    writeEvent('request', entry)
    printApiSummary('REQ', params.request?.method, params.request?.url)
    return
  }

  if (method === 'Network.requestWillBeSentExtraInfo') {
    writeEvent('request_extra', {
      request_id: params.requestId,
      target,
      headers: redactHeaders(params.headers),
      associated_cookies: redactValue(params.associatedCookies),
    })
    return
  }

  if (method === 'Network.responseReceived') {
    const key = requestKey(sessionId, params.requestId)
    const previous = requests.get(key) || {}
    requests.set(key, {
      ...previous,
      response: params.response,
      type: params.type || previous.type,
    })
    writeEvent('response', {
      request_id: params.requestId,
      target,
      type: params.type,
      response: redactResponse(params.response),
    })
    printApiSummary('RES', params.response?.status, params.response?.url)
    return
  }

  if (method === 'Network.loadingFinished') {
    await captureResponseBody(sessionId, params.requestId)
    requests.delete(requestKey(sessionId, params.requestId))
    return
  }

  if (method === 'Network.loadingFailed') {
    writeEvent('loading_failed', {
      request_id: params.requestId,
      target,
      error_text: params.errorText,
      canceled: params.canceled,
      blocked_reason: params.blockedReason,
    })
    requests.delete(requestKey(sessionId, params.requestId))
    return
  }

  if (method === 'Network.eventSourceMessageReceived') {
    writeEvent('event_source_message', {
      request_id: params.requestId,
      target,
      event_name: params.eventName,
      event_id: params.eventId,
      data: redactPayload(params.data),
    })
    return
  }

  if (method.startsWith('Network.webSocket')) {
    writeEvent(webSocketEventName(method), {
      request_id: params.requestId,
      target,
      url: redactUrl(params.url),
      request: redactValue(params.request),
      response: redactValue(params.response),
      frame: redactWebSocketFrame(params.response),
      error_message: params.errorMessage,
    })
  }
}

async function enableTarget(sessionId, targetInfo) {
  if (!sessionId || !shouldCaptureTarget(targetInfo)) return
  sessions.set(sessionId, publicTarget(targetInfo))
  await send(
    'Network.enable',
    {
      maxTotalBufferSize: 100 * 1024 * 1024,
      maxResourceBufferSize: 10 * 1024 * 1024,
      maxPostDataSize: 10 * 1024 * 1024,
    },
    sessionId
  )
  writeEvent('target_attached', { session_id: sessionId, target: publicTarget(targetInfo) })
}

async function captureResponseBody(sessionId, requestId) {
  const request = requests.get(requestKey(sessionId, requestId))
  if (!request?.response || !shouldCaptureBody(request)) return
  try {
    const result = await send('Network.getResponseBody', { requestId }, sessionId)
    const byteLength = Buffer.byteLength(
      result.body || '',
      result.base64Encoded ? 'base64' : 'utf8'
    )
    if (byteLength > MAX_TEXT_BODY_BYTES) {
      writeEvent('response_body_skipped', {
        request_id: requestId,
        target: request.target,
        reason: 'body_too_large',
        byte_length: byteLength,
      })
      return
    }
    writeEvent('response_body', {
      request_id: requestId,
      target: request.target,
      url: redactUrl(request.rawUrl),
      body: result.base64Encoded ? '[base64 body omitted]' : redactPayload(result.body),
      base64_encoded: Boolean(result.base64Encoded),
    })
  } catch (error) {
    writeEvent('response_body_unavailable', {
      request_id: requestId,
      target: request.target,
      message: errorMessage(error),
    })
  }
}

function shouldCaptureBody(request) {
  const mime = String(request.response?.mimeType || '').toLowerCase()
  const type = String(request.type || '')
  if (/^(image|video|audio)\//.test(mime) || /octet-stream|font|zip|pdf/.test(mime)) return false
  return ['XHR', 'Fetch', 'EventSource', 'Document', 'Other'].includes(type)
}

function shouldCaptureTarget(target = {}) {
  return ['page', 'other', 'iframe', 'worker', 'service_worker', 'shared_worker'].includes(
    target.type
  )
}

function hasTarget(targetId) {
  return [...sessions.values()].some((target) => target.target_id === targetId)
}

function publicTarget(target = {}) {
  return {
    target_id: target.targetId,
    type: target.type,
    title: target.title,
    url: redactUrl(target.url),
  }
}

function redactRequest(request = {}) {
  return {
    url: redactUrl(request.url),
    method: request.method,
    headers: redactHeaders(request.headers),
    post_data: redactPayload(request.postData),
    has_post_data: request.hasPostData,
    mixed_content_type: request.mixedContentType,
    referrer_policy: request.referrerPolicy,
  }
}

function redactResponse(response = {}) {
  return {
    url: redactUrl(response.url),
    status: response.status,
    status_text: response.statusText,
    headers: redactHeaders(response.headers),
    mime_type: response.mimeType,
    protocol: response.protocol,
    remote_ip_address: response.remoteIPAddress,
    remote_port: response.remotePort,
    from_disk_cache: response.fromDiskCache,
    from_service_worker: response.fromServiceWorker,
  }
}

function redactHeaders(headers = {}) {
  return Object.fromEntries(
    Object.entries(headers).map(([key, value]) => [
      key,
      secretKeyPattern.test(key) ? '[REDACTED]' : value,
    ])
  )
}

function redactUrl(value) {
  if (!value || typeof value !== 'string') return value
  try {
    const url = new URL(value)
    for (const key of url.searchParams.keys()) {
      if (secretKeyPattern.test(key)) url.searchParams.set(key, '[REDACTED]')
    }
    return url.toString()
  } catch {
    return value
  }
}

function redactPayload(value) {
  if (typeof value !== 'string' || !value) return value
  try {
    return redactValue(JSON.parse(value))
  } catch {
    return value
      .replace(
        /((?:msToken|a_bogus|sessionid|passport_csrf_token|access_token)=)[^&\s]+/gi,
        '$1[REDACTED]'
      )
      .replace(
        /("(?:msToken|a_bogus|sessionid|passport_csrf_token|access_token)"\s*:\s*")[^"]+/gi,
        '$1[REDACTED]'
      )
  }
}

function redactValue(value) {
  if (Array.isArray(value)) return value.map(redactValue)
  if (!value || typeof value !== 'object') return value
  return Object.fromEntries(
    Object.entries(value).map(([key, child]) => [
      key,
      secretKeyPattern.test(key) ? '[REDACTED]' : redactValue(child),
    ])
  )
}

function redactWebSocketFrame(frame) {
  if (!frame) return undefined
  return {
    opcode: frame.opcode,
    mask: frame.mask,
    payload_data: frame.opcode === 2 ? '[binary frame omitted]' : redactPayload(frame.payloadData),
  }
}

function webSocketEventName(method) {
  return method.replace('Network.', '').replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`)
}

function printApiSummary(kind, detail, url) {
  if (!url || !/doubao|bytedance|byteimg|volc|byteoversea/i.test(url)) return
  let pathname = url
  try {
    const parsed = new URL(url)
    pathname = `${parsed.host}${parsed.pathname}`
  } catch {}
  // 控制台只显示接口流量；静态资源仍完整保留在 JSONL 中。
  if (!/\/(?:chat|alice|samantha|im|biz|passport|top|upload|service)\//i.test(pathname)) return
  process.stdout.write(`${kind} ${detail ?? ''} ${pathname}\n`)
}

function requestKey(sessionId, requestId) {
  return `${sessionId}:${requestId}`
}

function writeEvent(type, data) {
  fs.appendFileSync(
    outputPath,
    `${JSON.stringify({ captured_at: new Date().toISOString(), event_type: type, ...redactValue(data) })}\n`
  )
}

function send(method, params = {}, sessionId) {
  const id = ++commandId
  const payload = { id, method, params }
  if (sessionId) payload.sessionId = sessionId
  websocket.send(JSON.stringify(payload))
  return new Promise((resolve, reject) => pendingCommands.set(id, { resolve, reject, method }))
}

function settleCommand(message) {
  const pending = pendingCommands.get(message.id)
  if (!pending) return
  pendingCommands.delete(message.id)
  if (message.error) {
    pending.reject(new Error(`${pending.method}: ${message.error.message}`))
  } else {
    pending.resolve(message.result || {})
  }
}

function getJson(requestPath) {
  return new Promise((resolve, reject) => {
    const request = http.get(
      { host: DEBUG_HOST, port: DEBUG_PORT, path: requestPath },
      (response) => {
        let body = ''
        response.setEncoding('utf8')
        response.on('data', (chunk) => (body += chunk))
        response.on('end', () => {
          try {
            resolve(JSON.parse(body))
          } catch (error) {
            reject(new Error(`无法读取 CDP ${requestPath}: ${errorMessage(error)}`))
          }
        })
      }
    )
    request.on('error', reject)
  })
}

function defaultOutputPath() {
  const timestamp = new Date()
    .toISOString()
    .replace(/[:.]/g, '-')
    .replace('T', '_')
    .replace('Z', '')
  return path.resolve('logs', 'captures', `doubaowork-${timestamp}.jsonl`)
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error)
}

function fail(error) {
  process.stderr.write(`CAPTURE_FAILED ${errorMessage(error)}\n`)
  process.exitCode = 1
  websocket.close()
}

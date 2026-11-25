// 回调需要跨越登录准备和账号保存两次脚本执行；仅在授权期间占用本机端口。
const AG_REDIRECT = 'http://localhost:8085/callback'
const AG_RESULT = 'http://127.0.0.1:8085/result'
const AG_CALLBACK_WORKER = String.raw`
const http = require('node:http')
const { createHash, timingSafeEqual } = require('node:crypto')
const [address, expectedState, readerDigest] = process.argv.slice(1)
const callback = new URL(address)
let result
let polls = 0
let idleTimer
const pollLeaseMs = 12000
function stop() {
  server.close()
  server.closeAllConnections()
}
function renew(ms) {
  clearTimeout(idleTimer)
  idleTimer = setTimeout(stop, ms).unref()
}
const server = http.createServer({ requestTimeout: 5000, headersTimeout: 5000 }, (req, res) => {
  const reply = (status, body, type = 'text/plain; charset=utf-8') => {
    res.writeHead(status, { 'Content-Type': type, 'Cache-Control': 'no-store', 'Connection': 'close',
      'Referrer-Policy': 'no-referrer', 'X-Content-Type-Options': 'nosniff' })
    res.end(body)
  }
  if (![callback.host, '127.0.0.1:' + callback.port].includes(req.headers.host))
    return reply(400, '无效的回调地址')
  if (!req.url.startsWith('/') || req.url.startsWith('//')) return reply(400, '无效路径')
  let url
  try { url = new URL(req.url, callback.origin) } catch { return reply(400, '无效路径') }
  if (url.origin !== callback.origin) return reply(400, '无效路径')
  if (url.pathname === '/ready' && req.method === 'GET')
    return reply(200, JSON.stringify({instance:createHash('sha256').update(readerDigest).digest('hex'), polls}), 'application/json')
  if (url.pathname === '/result' && req.method === 'POST') {
    const digest = createHash('sha256').update(req.headers.authorization || '').digest('hex')
    if (!timingSafeEqual(Buffer.from(digest), Buffer.from(readerDigest))) return reply(403, '禁止访问')
    if (url.searchParams.get('close') === '1') {
      server.close()
      reply(200, '{}', 'application/json')
      return
    }
    polls++
    renew(pollLeaseMs)
    return reply(200, JSON.stringify(result || { pending: true }), 'application/json')
  }
  if (req.method !== 'GET' || url.pathname !== callback.pathname) return reply(404, '未知路径')
  const p = url.searchParams
  if (p.getAll('state').length !== 1 || p.get('state') !== expectedState ||
      (p.has('iss') && p.get('iss') !== 'https://accounts.google.com')) return reply(400, '授权会话不匹配')
  const code = p.get('code'), error = p.get('error')
  if (p.getAll('code').length + p.getAll('error').length !== 1 || (!code && !error))
    return reply(400, '无效的授权回调')
  if (!result) result = code ? { code } : { denied: true }
  reply(200, result.code ? 'MediaGen 已收到授权回调，请返回账号窗口保存。' : '授权已取消，请返回 MediaGen。')
})
server.once('error', () => process.exit(2))
server.listen(Number(callback.port), '127.0.0.1')
renew(45000)
setTimeout(stop, 600000).unref()
`
const AG_CALLBACK_LAUNCHER = String.raw`
const { spawn } = require('node:child_process')
const child = spawn(process.execPath, ['-e', ...process.argv.slice(1)], {
  detached: true, windowsHide: true, stdio: 'ignore', env: {}
})
child.once('error', () => process.exit(1))
child.unref()
`

function agReady() {
  return host.tryCall('http.request', {
    method: 'GET',
    url: 'http://127.0.0.1:8085/ready',
    proxy: 'direct',
    timeout_ms: 300,
    follow_redirects: false,
    retry: { max_attempts: 1 },
  })
}

// 窗口每三秒查询一次授权状态。旧窗口停止查询时等待其释放，避免抢占其他活跃登录。
function agWaitForPrevious() {
  const previous = agReady()
  if (!previous.success) return
  const worker = previous.data?.body
  if (
    previous.data.ok &&
    /^[a-f0-9]{64}$/.test(worker?.instance) &&
    Number.isSafeInteger(worker.polls)
  ) {
    const deadline = Date.now() + 46000
    while (Date.now() < deadline) {
      host.call('time.sleep', { milliseconds: 250 })
      const ready = agReady()
      if (!ready.success) return
      if (
        ready.data?.body?.instance !== worker.instance ||
        ready.data?.body?.polls !== worker.polls
      )
        break
    }
  }
  throw new Error('本地回调端口 8085 已被占用，请完成已有登录或检查端口')
}

function agListen(profileDir, state, reader) {
  const digest = host.call('crypto.sha256_hex', { text: 'Bearer ' + reader }).value
  const expected = host.call('crypto.sha256_hex', { text: digest }).value
  const args = ['-e', AG_CALLBACK_LAUNCHER, AG_CALLBACK_WORKER, AG_REDIRECT, state, digest]
  const windows = /^[A-Za-z]:[\\/]/.test(profileDir)
  agWaitForPrevious()
  const child = host.tryCall('process.spawn', {
    command: windows ? 'C:\\Program Files\\nodejs\\node.exe' : '/usr/bin/env',
    args: windows ? args : ['node', ...args],
  })
  if (!child.success) throw new Error('本地 OAuth 回调需要 Node.js，启动失败')
  for (let i = 0; i < 30; i++) {
    const ready = agReady()
    if (ready.success) {
      if (ready.data?.ok && ready.data.body?.instance === expected) return
      throw new Error('本地回调端口 8085 已被占用，请完成已有登录或检查端口')
    }
    host.call('time.sleep', { milliseconds: 100 })
  }
  throw new Error('本地回调未就绪，请检查 Node.js 和 8085 端口')
}

function agCallback(reader, close = false) {
  return host.tryCall('http.request', {
    method: 'POST',
    url: AG_RESULT + (close ? '?close=1' : ''),
    proxy: 'direct',
    follow_redirects: false,
    headers: { Authorization: 'Bearer ' + reader },
    timeout_ms: 3000,
    retry: { max_attempts: 1 },
  })
}

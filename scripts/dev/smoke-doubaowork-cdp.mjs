#!/usr/bin/env node

import http from 'node:http'

const targetId = process.argv[2]
const kind = process.argv[3] || 'inspect'
if (!targetId)
  throw new Error('用法: smoke-doubaowork-cdp.mjs <target_id> <inspect|inspect-upload|ui-video>')

const target = (await getJson('/json/list')).find((item) => item.id === targetId)
if (!target) throw new Error(`找不到 CDP target: ${targetId}`)

const socket = new WebSocket(target.webSocketDebuggerUrl)
const pending = new Map()
let sequence = 0

await new Promise((resolve, reject) => {
  socket.addEventListener('open', resolve, { once: true })
  socket.addEventListener('error', reject, { once: true })
})

socket.addEventListener('message', (event) => {
  const message = JSON.parse(String(event.data))
  const command = pending.get(message.id)
  if (!command) return
  pending.delete(message.id)
  if (message.error) command.reject(new Error(message.error.message))
  else command.resolve(message.result || {})
})

const inspected = await evaluate(`({
  href: location.href,
  title: document.title,
  body_text_tail: (document.body?.innerText || '').slice(-12000),
  cookie_names: document.cookie.split(';').map(v => v.trim().split('=')[0]).filter(Boolean),
  storage_keys: Object.keys(localStorage),
  editors: [...document.querySelectorAll('[contenteditable="true"], textarea')].map((item, index) => ({
    index,
    tag: item.tagName,
    id: item.id,
    class_name: item.className,
    role: item.getAttribute('role'),
    placeholder: item.getAttribute('placeholder') || item.getAttribute('data-placeholder'),
  })),
  file_inputs: [...document.querySelectorAll('input[type="file"]')].map((item, index) => ({
    index,
    accept: item.accept,
    multiple: item.multiple,
  })),
  testids: [...document.querySelectorAll('[data-testid]')]
    .map(item => item.getAttribute('data-testid'))
    .filter(Boolean),
  composer_html: (() => {
    let root = document.querySelector('[contenteditable="true"]')
    while (root && !root.querySelector?.('#flow-end-msg-send')) root = root.parentElement
    return (root?.outerHTML || '').slice(0, 30000)
  })(),
  send_context_html: (document.querySelector('#flow-end-msg-send')?.parentElement?.parentElement?.parentElement?.outerHTML || '').slice(0, 30000),
  buttons: [...document.querySelectorAll('button')].slice(-40).map((item, index) => ({
    index,
    id: item.id,
    title: item.title,
    aria_label: item.getAttribute('aria-label'),
    text: (item.textContent || '').trim().slice(0, 80),
    disabled: item.disabled,
  }))
})`)

if (kind === 'inspect') {
  process.stdout.write(`${JSON.stringify(inspected, null, 2)}\n`)
  socket.close()
  process.exit(0)
}

if (kind === 'ui-video') {
  const video = await runUiVideoSmoke()
  process.stdout.write(`${JSON.stringify({ inspected, kind, result: video }, null, 2)}\n`)
  socket.close()
  process.exit(0)
}

if (kind === 'inspect-upload') {
  const point = await evaluate(`(() => {
    const item = document.querySelector('[data-testid="upload_file_button"]')
    const rect = item?.getBoundingClientRect()
    return rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null
  })()`)
  if (!point) throw new Error('找不到上传按钮')
  await command(
    'Input.dispatchMouseEvent',
    { type: 'mousePressed', x: point.x, y: point.y, button: 'left', clickCount: 1 },
    30_000
  )
  await command(
    'Input.dispatchMouseEvent',
    { type: 'mouseReleased', x: point.x, y: point.y, button: 'left', clickCount: 1 },
    30_000
  )
  await new Promise((resolve) => setTimeout(resolve, 800))
  const upload = await evaluate(`({
    text: (document.body.innerText || '').slice(-4000),
    file_inputs: [...document.querySelectorAll('input[type="file"]')].map(input => ({
      accept: input.accept,
      multiple: input.multiple,
    })),
    options: [...document.querySelectorAll('[role="menuitem"], [data-radix-collection-item], button')]
      .map(item => (item.textContent || '').trim())
      .filter(Boolean)
      .slice(-50),
    upload_options: [...document.querySelectorAll('*')]
      .filter(item => ['添加文件或图片', '选择云盘文件'].includes((item.textContent || '').trim()))
      .map(item => ({ tag: item.tagName, role: item.getAttribute('role'), class_name: item.className })),
  })`)
  process.stdout.write(`${JSON.stringify(upload, null, 2)}\n`)
  socket.close()
  process.exit(0)
}

throw new Error(`不支持的冒烟测试类型: ${kind}`)

function evaluate(expression, awaitPromise = false, timeoutMs = 30_000) {
  return command(
    'Runtime.evaluate',
    { expression, awaitPromise, returnByValue: true },
    timeoutMs
  ).then((result) => {
    if (result.exceptionDetails) {
      const message = result.exceptionDetails.exception?.description || result.exceptionDetails.text
      throw new Error(message)
    }
    return result.result?.value
  })
}

async function runUiVideoSmoke() {
  const before = await uiSnapshot()
  const text =
    '[Seedance 2.5](skill://seedance-25?type=2&id=353376777986) 原始 prompt：一只纸折小船在清澈溪流中缓慢前行。时长5秒，比例16:9。'
  const waitingForConfirmation = /是否按以上信息|是否使用 Seedance 2\.5 生成/.test(before.text)
  await insertComposerText(
    waitingForConfirmation ? '确认，按以上信息使用 Seedance 2.5 生成。' : text
  )
  const submitted = await evaluate(`(() => {
    const button = document.querySelector('#flow-end-msg-send')
    if (!button || button.disabled) return false
    button.click()
    return true
  })()`)
  if (!submitted) throw new Error('Doubao Work 发送按钮尚未就绪')

  let confirmationSent = waitingForConfirmation
  const deadline = Date.now() + 15 * 60 * 1000
  while (Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 5000))
    const snapshot = await uiSnapshot()
    const newVideos = snapshot.videos.filter((url) => !before.videos.includes(url))
    if (newVideos.length)
      return { status: 'done', videos: newVideos.map((video_url) => ({ video_url })) }
    if (/710022004|需要验证|rate limited/.test(snapshot.text)) {
      throw new Error('710022004: Doubao Work 需要完成风控验证')
    }
    if (/我暂时无法生成|未认证人脸暂不支持/.test(snapshot.text)) {
      throw new Error('Doubao Work 拒绝了本次视频生成')
    }
    if (!confirmationSent && /是否按以上信息|是否使用 Seedance 2\.5 生成/.test(snapshot.text)) {
      await insertComposerText('确认，按以上信息使用 Seedance 2.5 生成。')
      const confirmed = await evaluate(`(() => {
        const button = document.querySelector('#flow-end-msg-send')
        if (!button || button.disabled) return false
        button.click()
        return true
      })()`)
      if (!confirmed) throw new Error('确认消息发送按钮尚未就绪')
      confirmationSent = true
    }
  }
  throw new Error('Doubao Work UI 视频生成超过 15 分钟')
}

async function insertComposerText(text) {
  const focused = await evaluate(`(() => {
    const editor = document.querySelector('[contenteditable="true"]')
    if (!editor) return false
    editor.focus()
    document.execCommand('selectAll', false)
    document.execCommand('delete', false)
    return true
  })()`)
  if (!focused) throw new Error('找不到 Doubao Work 输入框')
  await command('Input.insertText', { text }, 30_000)
  await new Promise((resolve) => setTimeout(resolve, 800))
}

async function uiSnapshot() {
  return evaluate(`(() => ({
    text: (document.body.innerText || '').slice(-12000),
    permissionRequired: [...document.querySelectorAll('button')].some(button =>
      button.title === '全部允许' && !button.disabled
    ),
    videos: [...document.querySelectorAll('video, video source')]
      .map(item => item.currentSrc || item.src || '')
      .filter(url => /^https?:/.test(url)),
  }))()`)
}

function command(method, params, timeoutMs) {
  const id = ++sequence
  socket.send(JSON.stringify({ id, method, params }))
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id)
      reject(new Error(`${method} 超时`))
    }, timeoutMs)
    pending.set(id, {
      resolve: (value) => {
        clearTimeout(timer)
        resolve(value)
      },
      reject: (error) => {
        clearTimeout(timer)
        reject(error)
      },
    })
  })
}

function getJson(requestPath) {
  return new Promise((resolve, reject) => {
    const request = http.get({ host: '::1', port: 9222, path: requestPath }, (response) => {
      let body = ''
      response.setEncoding('utf8')
      response.on('data', (chunk) => (body += chunk))
      response.on('end', () => {
        try {
          resolve(JSON.parse(body))
        } catch (error) {
          reject(error)
        }
      })
    })
    request.on('error', reject)
  })
}

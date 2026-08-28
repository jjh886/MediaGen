#!/usr/bin/env node

import fs from 'node:fs/promises'
import http from 'node:http'
import vm from 'node:vm'

const target = await httpJson('/json/new?https%3A%2F%2Fwww.doubao.com%2Fchat%2F', 'PUT')
await new Promise((resolve) => setTimeout(resolve, 5000))

const image = await fs.readFile('apps/local-launcher/assets/mediagen.png')
const dataUrl = `data:image/png;base64,${image.toString('base64')}`
const pluginSource = await fs.readFile('plugins/doubaowork-api/scripts/external_ui.js', 'utf8')
const expression = vm.runInNewContext(
  `${pluginSource}\ndoubaoWorkUploadExpression(${JSON.stringify([dataUrl])})`
)
const result = await evaluate(target.webSocketDebuggerUrl, expression)
process.stdout.write(`${JSON.stringify(result, null, 2)}\n`)
await httpJson(`/json/close/${target.id}`)

async function evaluate(websocketUrl, source) {
  const socket = new WebSocket(websocketUrl)
  await new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true })
    socket.addEventListener('error', reject, { once: true })
  })
  const result = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('参考图上传冒烟测试超时')), 180000)
    socket.addEventListener('message', (event) => {
      const message = JSON.parse(String(event.data))
      if (message.id !== 1) return
      clearTimeout(timer)
      if (message.error) reject(new Error(message.error.message))
      else if (message.result?.exceptionDetails) {
        reject(
          new Error(
            message.result.exceptionDetails.exception?.description ||
              message.result.exceptionDetails.text
          )
        )
      } else resolve(message.result?.result?.value)
    })
    socket.send(
      JSON.stringify({
        id: 1,
        method: 'Runtime.evaluate',
        params: { expression: source, awaitPromise: true, returnByValue: true },
      })
    )
  })
  socket.close()
  return result
}

function httpJson(requestPath, method = 'GET') {
  return new Promise((resolve, reject) => {
    const request = http.request(
      { host: '::1', port: 9222, path: requestPath, method },
      (response) => {
        let body = ''
        response.setEncoding('utf8')
        response.on('data', (chunk) => (body += chunk))
        response.on('end', () => {
          if (body === 'Target is closing') resolve({ closed: true })
          else {
            try {
              resolve(JSON.parse(body))
            } catch (error) {
              reject(error)
            }
          }
        })
      }
    )
    request.on('error', reject)
    request.end()
  })
}

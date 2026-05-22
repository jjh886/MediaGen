#!/usr/bin/env node

import { spawn } from 'child_process'
import { existsSync } from 'fs'
import { mkdir, readFile, stat, writeFile } from 'fs/promises'
import { dirname, extname, resolve } from 'path'

const DEFAULT_MODEL = 'gpt-5.5'
const DEFAULT_REASONING_EFFORT = 'low'
const DEFAULT_SIZE = '1440x2560'
const DEFAULT_QUALITY = 'high'
const DEFAULT_BACKGROUND = 'auto'
const DEFAULT_FORMAT = 'png'
const DEFAULT_TIMEOUT_SECONDS = 1800
const DEFAULT_PROGRESS_INTERVAL_SECONDS = 15
const DEFAULT_MAX_ATTEMPTS = Infinity
const RETRY_DELAYS_SECONDS = [5, 15, 30, 60]
const MAX_IMAGE_BYTES = 50 * 1024 * 1024
const QUALITY_VALUES = ['auto', 'low', 'medium', 'high']
const FORMAT_VALUES = ['png', 'jpeg', 'webp']

const VALUE_FLAGS = new Set([
  '--prompt',
  '--out',
  '--size',
  '--quality',
  '--format',
  '--reference-image',
  '--edit-image',
])

const BOOL_FLAGS = new Set(['--help', '-h', '--force', '--optimize-prompt'])

function help() {
  console.log(`
图片生成命令

文字生图：~/bin/image-gen.mjs --prompt "A cozy alpine cabin at dawn" --out ~/Desktop/cabin.png
参考图：~/bin/image-gen.mjs --reference-image ~/Desktop/ref.png --prompt "Use it as identity reference" --out ~/Desktop/portrait.png

常用参数：
  --prompt           必填。图片描述或编辑要求，建议把人物、场景、风格、限制写清楚。
  --out              输出图片路径；不传则不保存文件，直接在 JSON 中返回 image_base64。
  --size             图片尺寸，传给 image_generation；默认 ${DEFAULT_SIZE}。
                     常用：1440x2560 竖图，2560x1440 横图，2048x2048 方图。
  --quality          图片渲染质量，可选：${QUALITY_VALUES.join(' / ')}，默认 ${DEFAULT_QUALITY}。
                     auto 自动选择；low 更快；medium 折中；high 更细致但更慢。
  --format           输出图片格式，可选：${FORMAT_VALUES.join(' / ')}，默认 ${DEFAULT_FORMAT}。
  --reference-image  参考图，可重复传多张，用于参考人脸、风格、服装、构图等。
  --edit-image       待编辑原图。传入后自动使用 edit，否则默认 generate。
  --force            如果输出文件已存在，允许覆盖。
  --optimize-prompt  默认不优化提示词；加上后会轻度整理提示词，但尽量保留原意。
  临时错误：遇到 retryable error 会一直自动重试，直到成功或出现不可重试错误。
`)
}

function die(message) {
  console.error(`Error: ${message}`)
  process.exit(1)
}

function parseArgs(argv) {
  const options = { reference_image: [] }

  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i]

    if (BOOL_FLAGS.has(flag)) {
      if (flag === '--help' || flag === '-h') options.help = true
      else options[toKey(flag)] = true
      continue
    }

    if (!VALUE_FLAGS.has(flag)) die(`未知参数：${flag}`)
    const value = argv[i + 1]
    if (!value || value.startsWith('--')) die(`${flag} 后面需要跟一个值`)
    i += 1

    const key = toKey(flag)
    if (key === 'reference_image') options.reference_image.push(value)
    else options[key] = value
  }

  return options
}

function toKey(flag) {
  return flag.replace(/^--/, '').replaceAll('-', '_')
}

async function readPrompt(options) {
  if (options.prompt) return options.prompt.trim()
  die('缺少提示词，请传 --prompt。')
}

function resolveHome(path) {
  if (path.startsWith('~/')) return resolve(process.env.HOME || '.', path.slice(2))
  return resolve(path)
}

function validateOptions(options) {
  const quality = options.quality || DEFAULT_QUALITY
  const format = options.format || DEFAULT_FORMAT

  if (!QUALITY_VALUES.includes(quality)) {
    die(`--quality 只能是 ${QUALITY_VALUES.join('、')}。`)
  }
  if (!FORMAT_VALUES.includes(format)) {
    die(`--format 只能是 ${FORMAT_VALUES.join('、')}。`)
  }
}

async function imageContentItem(path, roleText) {
  const absolute = resolveHome(path)
  await assertImageFile(absolute)
  const bytes = await readFile(absolute)

  return [
    {
      type: 'input_text',
      text: roleText,
    },
    {
      type: 'input_image',
      image_url: `data:${mimeType(absolute)};base64,${bytes.toString('base64')}`,
    },
  ]
}

async function assertImageFile(path) {
  if (!existsSync(path)) die(`图片不存在：${path}`)
  const info = await stat(path)
  if (info.size > MAX_IMAGE_BYTES) die(`图片超过 50MB：${path}`)
}

function mimeType(path) {
  const ext = extname(path).toLowerCase()
  if (ext === '.png') return 'image/png'
  if (ext === '.jpg' || ext === '.jpeg') return 'image/jpeg'
  if (ext === '.webp') return 'image/webp'
  return 'application/octet-stream'
}

async function buildPayload(options, prompt) {
  const content = []

  if (options.edit_image) {
    content.push(
      ...(await imageContentItem(
        options.edit_image,
        'The next image is the edit target. Preserve identity and unchanged areas unless the prompt says otherwise.'
      ))
    )
  }

  for (const ref of options.reference_image) {
    content.push(
      ...(await imageContentItem(
        ref,
        'The next image is a reference image. Use it only for identity, style, composition, or visual guidance.'
      ))
    )
  }

  content.push({ type: 'input_text', text: prompt })

  return {
    model: DEFAULT_MODEL,
    instructions: [
      'Use the image_generation tool to create the requested image.',
      options.optimize_prompt
        ? "Lightly optimize the prompt for image generation while preserving the user's intent and constraints."
        : "Do not rewrite, expand, polish, or add creative details to the user's prompt.",
      'Preserve the user intent, exact text, subject, style, composition, and constraints.',
      'Return the image generation result.',
    ].join(' '),
    input: [{ role: 'user', content }],
    reasoning: { effort: DEFAULT_REASONING_EFFORT },
    tools: [
      {
        type: 'image_generation',
        size: options.size || DEFAULT_SIZE,
        quality: options.quality || DEFAULT_QUALITY,
        background: DEFAULT_BACKGROUND,
        output_format: options.format || DEFAULT_FORMAT,
        action: options.edit_image ? 'edit' : 'generate',
      },
    ],
    tool_choice: { type: 'image_generation' },
    store: false,
    stream: true,
  }
}

function sleep(ms) {
  return new Promise((resolveSleep) => setTimeout(resolveSleep, ms))
}

function compactError(error) {
  return String(error?.message || error)
    .replace(/\s+/g, ' ')
    .trim()
}

function shouldRetry(error) {
  const message = compactError(error).toLowerCase()

  // 安全拦截、参数错误、模型不支持等问题，重试也不会变好，直接交给用户处理。
  if (
    message.includes('safety_violations') ||
    message.includes('safety system') ||
    message.includes('bad request') ||
    message.includes('unsupported') ||
    message.includes('invalid')
  ) {
    return false
  }

  return [
    'retryable error',
    'you can retry',
    'timeout',
    'timed out',
    '生成超时',
    'econnreset',
    'econnrefused',
    'enotfound',
    'eai_again',
    'socket hang up',
    'rate limit',
    'overloaded',
    'http 429',
    'http 500',
    'http 502',
    'http 503',
    'http 504',
  ].some((pattern) => message.includes(pattern))
}

function makeProgressReporter(payload, outPath, attempt) {
  const startedAt = Date.now()
  const seen = new Set()
  let heartbeat = null

  const elapsed = () => `${Math.floor((Date.now() - startedAt) / 1000)}s`
  const log = (message) => console.error(`[${elapsed()}] ${message}`)
  const once = (key, message) => {
    if (seen.has(key)) return
    seen.add(key)
    log(message)
  }

  return {
    start() {
      const tool = payload.tools[0]
      const maxLabel = DEFAULT_MAX_ATTEMPTS === Infinity ? '∞' : DEFAULT_MAX_ATTEMPTS
      log(
        `第 ${attempt}/${maxLabel} 次尝试：${tool.size} / ${tool.quality} / background=${tool.background}`
      )
      log(outPath ? `输出文件：${outPath}` : '输出方式：返回 base64，不保存文件。')
      heartbeat = setInterval(() => {
        log('仍在等待 返回图片，请稍等...')
      }, DEFAULT_PROGRESS_INTERVAL_SECONDS * 1000)
    },
    event(event) {
      const item = event.item || {}
      if (event.type === 'response.created') once('created', '已创建响应。')
      if (event.type === 'response.in_progress') once('in_progress', '正在处理请求。')
      if (event.type === 'response.completed') once('completed', '响应已完成。')
      if (event.type === 'response.failed') once('failed', '响应失败。')

      if (event.type === 'response.output_item.added' && item.type === 'image_generation_call') {
        once('image_start', '图片生成工具已启动。')
      }
      if (event.type === 'response.output_item.done' && item.type === 'image_generation_call') {
        once('image_done', '图片数据已返回，准备解码保存。')
      }
    },
    stderr() {},
    saving() {
      once('saving', '正在写入本地图片文件。')
    },
    done() {
      this.stopHeartbeat()
      log(`全部完成，总耗时 ${elapsed()}。`)
    },
    stopHeartbeat() {
      if (heartbeat) clearInterval(heartbeat)
      heartbeat = null
    },
  }
}

async function runResponses(payload, timeoutSeconds, progress) {
  return new Promise((resolveRun, rejectRun) => {
    const child = spawn('codex', ['-c', 'mcp_servers={}', 'responses'], {
      stdio: ['pipe', 'pipe', 'pipe'],
    })

    let stdout = ''
    let stderr = ''
    let lineBuffer = ''

    // responses 输出的是 JSONL 事件流，不是普通的一整个 JSON。
    child.stdout.setEncoding('utf8')
    child.stderr.setEncoding('utf8')
    progress.start()
    child.stdout.on('data', (chunk) => {
      stdout += chunk
      lineBuffer += chunk

      const lines = lineBuffer.split(/\r?\n/)
      lineBuffer = lines.pop() || ''
      for (const line of lines) {
        if (!line.trim()) continue
        try {
          progress.event(JSON.parse(line))
        } catch {
          // 进度解析失败不影响最终结果；最终还会完整解析 stdout。
        }
      }
    })
    child.stderr.on('data', (chunk) => {
      stderr += chunk
      progress.stderr(chunk)
    })

    const timer = setTimeout(() => {
      progress.stopHeartbeat()
      child.kill('SIGTERM')
      rejectRun(new Error(`生成超时：${timeoutSeconds} 秒`))
    }, timeoutSeconds * 1000)

    child.on('error', (error) => {
      clearTimeout(timer)
      progress.stopHeartbeat()
      rejectRun(error)
    })

    child.on('close', (code) => {
      clearTimeout(timer)
      progress.stopHeartbeat()
      if (code !== 0) {
        rejectRun(new Error(stderr || stdout || `responses 退出码：${code}`))
        return
      }
      resolveRun({ stdout, stderr })
    })

    child.stdin.end(JSON.stringify(payload))
  })
}

async function runResponsesWithRetry(payload, timeoutSeconds, outPath) {
  let lastError = null

  for (let attempt = 1; attempt <= DEFAULT_MAX_ATTEMPTS; attempt += 1) {
    const progress = makeProgressReporter(payload, outPath, attempt)

    try {
      const result = await runResponses(payload, timeoutSeconds, progress)
      const info = extractImage(result.stdout)
      if (!info.imageBase64) die('没有找到 image_generation_call.result。')
      return { info, progress }
    } catch (error) {
      lastError = error
      progress.stopHeartbeat()

      if (!shouldRetry(error) || attempt === DEFAULT_MAX_ATTEMPTS) break

      const delay = RETRY_DELAYS_SECONDS[attempt - 1] || RETRY_DELAYS_SECONDS.at(-1)
      const maxLabel = DEFAULT_MAX_ATTEMPTS === Infinity ? '∞' : DEFAULT_MAX_ATTEMPTS
      console.error(`第 ${attempt}/${maxLabel} 次失败：${compactError(error)}`)
      console.error(`${delay}s 后自动重试...`)
      await sleep(delay * 1000)
    }
  }

  throw lastError
}

function extractImage(eventsText) {
  let imageBase64 = ''
  let revisedPrompt = ''
  let usage = null

  for (const line of eventsText.split(/\r?\n/)) {
    if (!line.trim()) continue

    try {
      const event = JSON.parse(line)
      const item = event.item || {}

      if (event.type === 'response.output_item.done' && item.type === 'image_generation_call') {
        imageBase64 = item.result || imageBase64
        revisedPrompt = item.revised_prompt || revisedPrompt
      }

      if (event.type === 'response.completed') {
        usage = event.response?.usage || usage
      }
    } catch {
      // JSONL 中如果混入非 JSON 行，跳过即可，不能影响图片提取。
    }
  }

  return { imageBase64, revisedPrompt, usage }
}

function outputPath(options) {
  if (!options.out) return null
  const format = options.format || DEFAULT_FORMAT
  const raw = resolveHome(options.out)
  if (extname(raw)) return raw
  return `${raw}.${format}`
}

function assertCanWriteOutput(options, outPath) {
  // 生成前先拦截覆盖风险，避免图片已生成才发现不能写入，白白消耗额度。
  if (outPath && existsSync(outPath) && !options.force) {
    die(`输出文件已存在：${outPath}。覆盖请加 --force。`)
  }
}

async function saveOutput(options, info, outPath, progress) {
  if (outPath) {
    await mkdir(dirname(outPath), { recursive: true })
    progress.saving()
    await writeFile(outPath, Buffer.from(info.imageBase64, 'base64'))
  }

  const response = buildResponse(options, info, outPath)
  progress.done()
  console.log(JSON.stringify(response, null, 2))
}

function buildResponse(options, info, outPath) {
  const response = {
    success: true,
    model: DEFAULT_MODEL,
    reasoning_effort: DEFAULT_REASONING_EFFORT,
    size: options.size || DEFAULT_SIZE,
    quality: options.quality || DEFAULT_QUALITY,
    background: DEFAULT_BACKGROUND,
    format: options.format || DEFAULT_FORMAT,
    action: options.edit_image ? 'edit' : 'generate',
    optimize_prompt: Boolean(options.optimize_prompt),
    reference_images: options.reference_image.map(resolveHome),
    edit_image: options.edit_image ? resolveHome(options.edit_image) : null,
    revised_prompt: info.revisedPrompt || null,
    usage: info.usage,
    created_at: new Date().toISOString(),
  }
  if (outPath) response.image_path = outPath
  else response.image_base64 = info.imageBase64
  return response
}

async function main() {
  const options = parseArgs(process.argv.slice(2))
  if (options.help) return help()

  validateOptions(options)
  const prompt = await readPrompt(options)
  const outPath = outputPath(options)
  assertCanWriteOutput(options, outPath)

  const payload = await buildPayload(options, prompt)
  const { info, progress } = await runResponsesWithRetry(payload, DEFAULT_TIMEOUT_SECONDS, outPath)
  await saveOutput(options, info, outPath, progress)
}

main().catch((error) => die(error?.message || String(error)))

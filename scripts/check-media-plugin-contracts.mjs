#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'

const root = process.cwd()
const index = readJson('plugins/plugins.index.json')
const required = ['doubao-api', 'doubaowork-api']

for (const platform of required) {
  assert(index.plugins.includes(platform), `${platform} 没有加入 plugins.index.json`)
}

const doubao = readJson('plugins/doubao-api/manifest.json')
assert(doubao.adapter_mode === 'browser', 'doubao-api 必须使用浏览器内 fetch 保留签名指纹')
assertModel(doubao, 'doubao-image', 'image')
assertModel(doubao, 'doubao-video', 'video')
assertFlow(doubao, 'generate_image')
assertFlow(doubao, 'generate_video')

const work = readJson('plugins/doubaowork-api/manifest.json')
assert(work.adapter_mode === 'api', 'doubaowork-api 必须通过本机 App CDP 原子能力执行')
assertModel(work, 'doubaowork-seedream-5.0-pro', 'image')
assertModel(work, 'doubaowork-seedance-2.5', 'video')
assertFlow(work, 'generate_image')
assertFlow(work, 'generate_video')
assert(work.video_capabilities.max_duration_seconds === 30, 'Seedance 2.5 最长时长应为 30 秒')

compileFlowScripts(doubao)
compileFlowScripts(work)
assertDoubaoErrorsAreSanitized()
assertDoubaoWorkRuntimeExpressions()
process.stdout.write('✅ 豆包与 Doubao Work 插件契约检查通过。\n')

function assertDoubaoErrorsAreSanitized() {
  const context = vm.createContext({})
  const source = readScript('doubao-api', '@shared/doubao_internal_api.js')
  new vm.Script(source, { filename: 'doubao_internal_api.js' }).runInContext(context)
  const eventData = JSON.stringify({
    code: 710022004,
    message: 'rate limited',
    error_detail: { ext: { decision: 'private-decision-token' } },
  })
  let message = ''
  try {
    context.doubaoInternalThrowEventError([{ event_type: 2005, event_data: eventData }], '失败')
  } catch (error) {
    message = String(error)
  }
  assert(message.includes('710022004'), '豆包错误必须保留可分类的稳定错误码')
  assert(!message.includes('private-decision-token'), '豆包错误不能暴露内部风控 decision.detail')
}

function assertDoubaoWorkRuntimeExpressions() {
  const source = readScript('doubaowork-api', 'external_ui.js')
  const host = {
    call(action, payload) {
      if (action !== 'external_cdp.evaluate') return null
      new vm.Script(payload.expression, { filename: 'doubaowork-cdp-expression.js' })
      return { text: '', images: [], videos: [] }
    },
  }
  const context = vm.createContext({ host })
  new vm.Script(source, { filename: 'doubaowork-external-ui.js' }).runInContext(context)
  context.doubaoWorkUiSnapshot('contract-target')
  assert(
    context.doubaoWorkResultError(
      'Seedance 2.5 为会员专属模型，当前账号暂未开通加强版本会员或高级版本会员',
      'video'
    ),
    'Doubao Work 必须识别实际返回的 Seedance 2.5 会员权限错误'
  )
}

function compileFlowScripts(manifest) {
  for (const [flowName, flow] of Object.entries(manifest.flows || {})) {
    for (const step of flow.steps || []) {
      const references = step.script_files || (step.script_file ? [step.script_file] : [])
      if (!references.length) continue
      const source = references
        .map((reference) => readScript(manifest.platform_id, reference))
        .join('\n;\n')
      assert(!source.includes('{{'), `${manifest.platform_id}.${flowName} 脚本仍有未解析模板`)
      new vm.Script(source, { filename: `${manifest.platform_id}:${flowName}` })
    }
  }
}

function readScript(platform, reference) {
  const file = reference.startsWith('@shared/')
    ? path.join(root, 'plugins/_shared', reference.slice('@shared/'.length))
    : path.join(root, 'plugins', platform, 'scripts', reference)
  return fs.readFileSync(file, 'utf8')
}

function assertModel(manifest, modelId, taskKind) {
  const model = manifest.models.find((item) => item.model_id === modelId)
  assert(model, `${manifest.platform_id} 缺少模型 ${modelId}`)
  assert(model.task_kind === taskKind, `${modelId} task_kind 应为 ${taskKind}`)
}

function assertFlow(manifest, flowName) {
  assert(manifest.flows?.[flowName]?.steps?.length > 0, `${manifest.platform_id} 缺少 ${flowName}`)
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(path.join(root, file), 'utf8'))
}

function assert(condition, message) {
  if (!condition) throw new Error(message)
}

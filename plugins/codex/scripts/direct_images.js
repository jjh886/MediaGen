// Codex 原生 Images 协议；模型、端点和回退条件全部由插件维护。
function codexImageModel(ctx) {
  const id = String(ctx.model_id || '')
  if (!id.startsWith('codex-gpt-image-'))
    throw new Error('unsupported_model: Codex 不支持图片模型 ' + id)
  return id.slice('codex-'.length)
}

function codexDirectImageRequest(payload) {
  const tool = payload.tools?.find((item) => item.type === 'image_generation')
  if (!tool?.model?.startsWith('gpt-image-')) return null
  const content = payload.input.flatMap((item) => item.content || [])
  const images = content.filter((part) => part.type === 'input_image')
  const texts = content.filter((part) => part.type === 'input_text')
  const body = { model: tool.model, prompt: texts.at(-1)?.text || '', stream: true }
  for (const key of [
    'size',
    'quality',
    'background',
    'output_format',
    'output_compression',
    'partial_images',
    'moderation',
    'input_fidelity',
  ]) {
    if (tool[key] !== undefined) body[key] = tool[key]
  }
  const edit = tool.action === 'edit' || images.length > 0
  if (edit) {
    if (!images.length) throw new Error('invalid_input: 编辑图片需要参考图')
    body.images = images.map((item) => ({ image_url: item.image_url }))
    if (tool.input_image_mask) body.mask = tool.input_image_mask
  }
  return {
    url: 'https://chatgpt.com/backend-api/codex/images/' + (edit ? 'edits' : 'generations'),
    body,
  }
}

function codexCanFallbackImageEndpoint(response, result) {
  if (result.image || ![404, 405].includes(Number(response.status))) return false
  const error = response.body?.error || {}
  // 模型无权限或不存在不能偷偷换模型，也不能当作接口不存在再提交。
  return !/model_not_found|unsupported_model|model.*(?:not found|does not exist|access)/i.test(
    [error.code, error.message, response.body_text, result.error].join(' ')
  )
}

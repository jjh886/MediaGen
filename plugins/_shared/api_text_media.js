// 通用 Responses 输出：平台插件只传入可展示的摘要和已确认的图片。
function apiTextReasoningContent(value) {
  return [
    ...new Set(
      String(value || '')
        .split(/\n+/)
        .map((line) => line.trim())
        .filter(
          (line) =>
            line &&
            !/^(?:(?:正在思考|思考中|思考完成|已完成思考|thinking|reasoning)[.…。\s]*|(?:思考了|已思考|思考用时|处理了|共用时)\s*(?:\d+(?:\.\d+)?\s*(?:小时|分钟|分|秒|毫秒)\s*)+[。.…\s]*|(?:thought|worked|reasoned)\s+for\s+(?:\d+(?:\.\d+)?\s*(?:seconds?|secs?|s|minutes?|mins?|m|hours?|h)\s*)+[.\s]*)+$/i.test(
              line
            )
        )
    ),
  ].join('\n')
}

function apiTextUpdateReasoning(writer, id, text, done) {
  writer.reasoning_items ||= {}
  let item = writer.reasoning_items[id]
  const incoming = apiTextReasoningContent(text)
  if (!incoming && !item) return
  if (!item) {
    item = { id: 'rs_' + apiTextUuid(), type: 'reasoning', status: 'in_progress', summary: [] }
    writer.reasoning_items[id] = item
    writer.output.push(item)
    apiTextEmitResponseEvent(writer, 'response.output_item.added', {
      output_index: writer.output.indexOf(item),
      item,
    })
  }
  const previous = item.summary.map((part) => part.text).join('')
  const current = incoming || previous
  item.summary = current ? [{ type: 'summary_text', text: current }] : []
  if (current.startsWith(previous) && current.length > previous.length) {
    apiTextEmitResponseEvent(writer, 'response.reasoning_summary_text.delta', {
      output_index: writer.output.indexOf(item),
      item_id: item.id,
      summary_index: 0,
      delta: current.slice(previous.length),
    })
  }
  item.status = done ? 'completed' : 'in_progress'
  apiTextEmitResponseEvent(
    writer,
    done ? 'response.output_item.done' : 'response.output_item.added',
    { output_index: writer.output.indexOf(item), item }
  )
}

function apiTextMediaProgress(writer, id, kind, status, progress) {
  writer.image_items ||= {}
  const key = kind + ':' + id
  let item = writer.image_items[key]
  if (!item) {
    item = {
      id: (kind === 'video' ? 'vg_' : 'ig_') + apiTextUuid(),
      type: kind + '_generation_call',
      status: 'in_progress',
      result: null,
    }
    writer.image_items[key] = item
    writer.output.push(item)
  }
  if (item.status === 'completed') return item
  item.status = status || 'in_progress'
  if (Number.isFinite(progress)) item.progress = Math.max(0, Math.min(100, progress))
  apiTextEmitResponseEvent(writer, 'response.output_item.added', {
    output_index: writer.output.indexOf(item),
    item,
  })
  return item
}

function apiTextImageProgress(writer, id, status, progress) {
  return apiTextMediaProgress(writer, id, 'image', status, progress)
}

function apiTextCompleteVideo(writer, id, url, metadata) {
  if (!/^https:\/\//.test(String(url || ''))) throw new Error('invalid_video: 平台视频地址无效')
  const item = apiTextMediaProgress(writer, id, 'video', 'generating')
  Object.assign(item, { status: 'completed', result_url: url, output_format: 'mp4', ...metadata })
  apiTextEmitResponseEvent(writer, 'response.output_item.done', {
    output_index: writer.output.indexOf(item),
    item,
  })
  return item
}

function apiTextCompleteImage(writer, id, dataUrl, metadata) {
  const item = apiTextImageProgress(writer, id, 'generating')
  const match = /^data:image\/(png|jpeg|webp);base64,([A-Za-z0-9+/=]+)$/.exec(dataUrl)
  if (!match) throw new Error('invalid_image: 平台图片返回格式无效')
  Object.assign(item, {
    status: 'completed',
    result: match[2],
    output_format: match[1] === 'jpeg' ? 'jpeg' : match[1],
    ...metadata,
  })
  apiTextEmitResponseEvent(writer, 'response.output_item.done', {
    output_index: writer.output.indexOf(item),
    item,
  })
  return item
}

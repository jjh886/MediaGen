// 对话中的媒体仍由普通文本请求触发；依据平台的媒体区块/生成回执展示状态。
function doubaoTextMediaState() {
  return {
    pending: '',
    creation_route: false,
    completed: {},
    user_message_id: '',
    reply_message_ids: [],
  }
}

function doubaoTextMediaEvent(session, state, writer, stream, event) {
  const data = apiSessionEventData(event)
  if (event.event === 'FULL_MSG_NOTIFY' && Number(data.message?.user_type) === 1) {
    state.user_message_id = data.message.message_id
    host.checkpoint({ ...host.checkpoint(), user_message_id: state.user_message_id })
  }
  if (event.event === 'STREAM_MSG_NOTIFY' && Number(data.meta?.user_type) === 2) {
    if (data.meta.message_id) state.reply_message_ids.push(data.meta.message_id)
    if (data.content?.ext?.use_creation === '1') state.creation_route = true
  }
  doubaoTextMediaBlocks(state, writer, stream.blocks)
  doubaoTextPendingMedia(state, writer, stream.text)
  if (state.pending) host.checkpoint({ ...host.checkpoint(), media_kind: state.pending })
}

function doubaoTextPendingMedia(state, writer, text) {
  if (
    state.creation_route &&
    !state.pending &&
    /生成好后|正在生成|预计等待|大约需要|开始生成|确认.{0,15}生成/.test(text)
  ) {
    if (/视频|Seedance/i.test(text)) state.pending = 'video'
    else if (/图片|图像|Seedream/i.test(text)) state.pending = 'image'
    if (state.pending) apiTextMediaProgress(writer, 'pending', state.pending, 'in_progress')
  }
}

function doubaoTextMediaBlocks(state, writer, blocks) {
  const thoughts = Object.values(blocks).filter(
    (block) => Number(block.block_type) === 10040 || block.content?.thinking_block
  )
  for (const thought of thoughts) {
    const texts = Object.values(blocks)
      .filter((block) => {
        const seen = new Set()
        let parent = block.parent_id
        while (parent && !seen.has(parent)) {
          if (parent === thought.block_id) return Number(block.block_type) === 10000
          seen.add(parent)
          parent = blocks[parent]?.parent_id
        }
        return false
      })
      .map((block) => block.content?.text_block?.text || '')
      .join('')
    const title = thought.content?.thinking_block?.title || ''
    if (texts || title) apiTextUpdateReasoning(writer, thought.block_id, texts || title, false)
  }
  for (const creation of Object.values(blocks).flatMap(
    (block) => block.content?.creation_block?.creations || []
  )) {
    const kind = creation.video ? 'video' : creation.image ? 'image' : ''
    if (!kind) continue
    state.pending = kind
    const key = doubaoTextCreationKey(creation)
    if (!state.completed[key])
      apiTextMediaProgress(writer, doubaoTextMediaOutputId(state, creation), kind, 'generating')
  }
}

function doubaoTextCreationKey(creation) {
  return String(creation.id || creation.video?.vid || creation.image?.key || '')
}

function doubaoTextMediaOutputId(state, creation) {
  const kind = creation.video ? 'video' : 'image'
  const key = doubaoTextCreationKey(creation)
  if (!key) return 'pending'
  state.output_ids ||= {}
  const identity = kind + ':' + key
  if (!state.output_ids[identity])
    state.output_ids[identity] = Object.keys(state.output_ids).some((id) =>
      id.startsWith(kind + ':')
    )
      ? key
      : 'pending'
  return state.output_ids[identity]
}

function doubaoCompleteTextMedia(session, state, writer, conversationId, creations) {
  for (const creation of creations) {
    const kind = creation.video ? 'video' : creation.image ? 'image' : ''
    const key = doubaoTextCreationKey(creation)
    if (!kind || !key || state.completed[key]) continue
    const id = doubaoTextMediaOutputId(state, creation)
    if (kind === 'image') {
      const raw = creation.image?.image_ori_raw
      if (!raw?.url || Number(creation.image.status) === 3) continue
      const result = apiSessionRequireOk(
        host.call('http.request', {
          method: 'GET',
          url: raw.url,
          proxy: 'system',
          response_body_format: 'base64',
          headers: {
            referer: 'https://www.doubao.com/',
            'user-agent': session.auth?.fingerprint?.user_agent || '',
          },
          timeout_ms: 120000,
        }),
        '读取豆包对话图片'
      )
      if (!result.body_base64) throw new Error('download_pending: 豆包对话图片尚未就绪')
      const mime = String(result.headers?.['content-type'] || 'image/png').split(';')[0]
      apiTextCompleteImage(writer, id, 'data:' + mime + ';base64,' + result.body_base64, {
        width: raw.width,
        height: raw.height,
      })
    } else {
      if (Number(creation.video.status) !== 3) continue
      const url = doubaoVideoDownload(session, creation.video.vid)
      apiTextCompleteVideo(writer, id, url, {
        duration: Number(creation.video.duration),
        provider_media_id: String(creation.video.vid),
      })
    }
    state.completed[key] = true
  }
  return Object.keys(state.completed).length
}

function doubaoTextTurnMessages(messages, state) {
  let userId = state.user_message_id || host.checkpoint()?.user_message_id
  const localId = host.checkpoint()?.local_message_id
  const user = messages.find(
    (message) =>
      (userId && message.message_id === userId) || (localId && message.local_message_id === localId)
  )
  if (!userId && user) {
    userId = user.message_id
    state.user_message_id = userId
  }
  const baseline = Number(user?.index_in_conv)
  return messages
    .filter(
      (message) =>
        Number(message.user_type) !== 1 &&
        ((userId && message.bot_reply_message_id === userId) ||
          state.reply_message_ids.includes(message.message_id) ||
          (Number.isFinite(baseline) && Number(message.index_in_conv) > baseline))
    )
    .sort((a, b) => Number(a.index_in_conv) - Number(b.index_in_conv))
}

function doubaoWaitForTextMedia(session, state, writer, conversationId) {
  const deadline = Date.now() + 60 * 60 * 1000
  while (Date.now() < deadline) {
    const messages = doubaoHistory(session, conversationId)
    const replies = doubaoTextTurnMessages(messages, state)
    if (replies.some((reply) => reply.ext?.use_creation === '1')) state.creation_route = true
    const blocks = Object.fromEntries(
      replies
        .flatMap((message) => message.content_block || [])
        .map((block) => [block.block_id, block])
    )
    for (const reply of replies) doubaoCheckCommerce(reply)
    const text = replies
      .map(
        (reply) =>
          (reply.content_block || [])
            .filter((block) => doubaoIsAnswerBlock(block, blocks))
            .map((block) => block.content?.text_block?.text || '')
            .join('') || doubaoLegacyText(reply)
      )
      .filter(Boolean)
      .join('\n\n')
    if (text.startsWith(writer.text))
      apiTextFeedResponseText(writer, text.slice(writer.text.length))
    doubaoTextPendingMedia(state, writer, text)
    doubaoTextMediaBlocks(state, writer, blocks)
    const creations = Object.values(blocks).flatMap(
      (block) => block.content?.creation_block?.creations || []
    )
    if (creations.some((creation) => Number(creation.image?.status) === 3))
      throw new Error('content_policy_refusal: 豆包未通过图片内容审核')
    const count = doubaoCompleteTextMedia(session, state, writer, conversationId, creations)
    if (
      count &&
      creations
        .filter((creation) => creation.image || creation.video)
        .every((creation) => state.completed[doubaoTextCreationKey(creation)])
    )
      return
    if (
      !state.pending &&
      !creations.length &&
      text.trim() &&
      replies.some((reply) => reply.ext?.is_finish === '1')
    )
      return
    if (
      state.pending &&
      !creations.length &&
      doubaoConfirmMediaIfRequested(session, conversationId, state.pending, replies)
    )
      continue
    if (
      creations.some(
        (creation) =>
          Number(creation.image?.status) < 0 ||
          Number(creation.image?.status) === 4 ||
          Number(creation.video?.status) === 4 ||
          Number(creation.video?.status) < 0
      )
    )
      throw new Error('platform_rejected: 豆包对话媒体生成失败')
    host.sleep(5000)
    doubaoHeartbeatTick(session)
  }
  throw new Error('generation_timeout: 豆包原媒体任务仍未完成，请稍后恢复')
}

function doubaoRefreshCompletedTextMedia(session, result) {
  for (const item of result?.response?.output || []) {
    if (item.type === 'video_generation_call' && item.provider_media_id)
      item.result_url = doubaoVideoDownload(session, item.provider_media_id)
  }
  return result
}

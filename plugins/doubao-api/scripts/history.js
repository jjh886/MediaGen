function doubaoHistory(session, conversationId) {
  const result = doubaoRequest(session, '/im/chain/single', {
    cmd: 3100,
    sequence_id: apiTextUuid(),
    channel: 2,
    version: '1',
    uplink_body: {
      pull_singe_chain_uplink_body: {
        conversation_id: conversationId,
        anchor_index: Number.MAX_SAFE_INTEGER,
        conversation_type: 3,
        direction: 1,
        limit: 20,
        ext: {},
        filter: { index_list: [] },
        evaluate_ab_params: '',
        evaluate_common_params: '',
        option: { lazy_load_strategy: 0 },
      },
    },
  })
  if (result.status_code !== 0) throw new Error('temporary_unavailable: 豆包会话回读失败')
  return result.downlink_body?.pull_singe_chain_downlink_body?.messages || []
}

function doubaoWaitForResult(session, conversationId, kind) {
  if (!/^[0-9]+$/.test(String(conversationId || '')))
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 豆包未返回原会话编号')
  const deadline = Date.now() + 60 * 60 * 1000
  let finishedWithoutMediaAt
  while (Date.now() < deadline) {
    const messages = doubaoHistory(session, conversationId)
    const assistant = messages.filter((message) => Number(message.user_type) !== 1)
    const creations = assistant.flatMap((message) =>
      (message.content_block || []).flatMap(
        (block) => block.content?.creation_block?.creations || []
      )
    )
    const completed = creations.filter((creation) =>
      kind === 'image'
        ? creation.image?.image_ori_raw?.url
        : creation.video?.vid && Number(creation.video.status) === 3
    )
    if (completed.length) return completed
    if (
      kind !== 'text' &&
      !creations.length &&
      doubaoConfirmMediaIfRequested(session, conversationId, kind, messages)
    ) {
      finishedWithoutMediaAt = undefined
      continue
    }
    if (kind === 'text') {
      const final = assistant.find((message) => message.ext?.is_finish === '1')
      const blocks = Object.fromEntries(
        (final?.content_block || []).map((block) => [block.block_id, block])
      )
      const text = (final?.content_block || [])
        .filter((block) => doubaoIsAnswerBlock(block, blocks))
        .map((block) => block.content?.text_block?.text || '')
        .join('')
      if (text.trim()) return text
    }
    if (
      kind !== 'text' &&
      !creations.length &&
      assistant.some((message) => message.ext?.is_finish === '1') &&
      !assistant.some((message) =>
        (message.content_block || []).some((block) =>
          /预计等待|正在生成|生成好后/.test(block.content?.text_block?.text || '')
        )
      )
    ) {
      // 网页会先结束文字回复，再通过异步消息追加生成卡片，不能把文字结束当作任务失败。
      finishedWithoutMediaAt ??= Date.now()
      if (Date.now() - finishedWithoutMediaAt >= 5 * 60 * 1000)
        throw new Error('generation_incomplete: 豆包尚未返回媒体卡片，请稍后恢复原任务')
    } else finishedWithoutMediaAt = undefined
    if (creations.some((creation) => Number(creation.image?.status) === 3))
      throw new Error('content_policy_refusal: 豆包未通过图片内容审核')
    const rejected = creations.some(
      (creation) =>
        Number(creation.image?.status) < 0 ||
        Number(creation.image?.status) === 4 ||
        Number(creation.video?.status) < 0 ||
        Number(creation.video?.status) === 4
    )
    if (rejected) throw new Error('platform_rejected: 豆包素材生成失败')
    host.sleep(5000)
  }
  throw new Error('generation_timeout: 豆包原任务仍未完成，请稍后恢复')
}

function doubaoConfirmMediaIfRequested(session, conversationId, kind, messages) {
  const latest = messages
    .filter((message) => Number(message.user_type) !== 1 && message.ext?.is_finish === '1')
    .sort((a, b) => Number(b.index_in_conv) - Number(a.index_in_conv))[0]
  if (!latest) return
  const reply = (latest.content_block || [])
    .map((block) => block.content?.text_block?.text || '')
    .join('\n')
  if (!/确认后.{0,20}(开始|生成)|请.{0,10}确认.{0,30}(参数|生成)|是否继续生成|确认生成/.test(reply))
    return
  if (/验证码|授权|使用规范|隐私|购买|升级|开通|充值|订阅|支付密码|银行卡/.test(reply))
    throw new Error('generation_requires_confirmation: 豆包要求额外授权，请在登录窗口处理')
  const saved = host.checkpoint() || {}
  const replies = saved.confirmation_replies || []
  const previous = replies.find((item) => item.source_message_id === latest.message_id)
  if (previous) {
    if (
      previous.status !== 'sent' &&
      !messages.some((message) => message.local_message_id === previous.local_message_id)
    )
      throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 原会话确认请求提交状态待核对，不能重复发送')
    return
  }
  if (replies.length >= 2)
    throw new Error('generation_requires_confirmation: 豆包重复要求确认，已停止自动回复')
  if (!latest.message_id) throw new Error('invalid_response: 豆包确认消息缺少编号')
  const info = doubaoImRequest(session, '/im/conversation/info', 1110, {
    get_conv_info_uplink_body: {
      conversation_id: conversationId,
      ext: { cold_start: 'true' },
      bot_id: '',
      conversation_type: 3,
      option: { need_bot_info: true },
    },
  })
  const conversation = info.downlink_body?.get_conv_info_downlink_body?.conversation_info
  if (String(conversation?.conversation_id) !== String(conversationId))
    throw new Error('invalid_response: 豆包确认请求的原会话不一致')
  const payload = doubaoConfirmationPayload(conversation, kind)
  const entry = {
    source_message_id: latest.message_id,
    local_message_id: payload.messages[0].local_message_id,
    status: 'submitting',
  }
  host.checkpoint({ ...saved, confirmation_replies: [...replies, entry] })
  doubaoSubmit(session, payload)
  entry.status = 'sent'
  host.checkpoint({
    ...host.checkpoint(),
    phase: 'accepted',
    confirmation_replies: [...replies, entry],
  })
  return true
}

function doubaoVideoDownload(session, vid) {
  const home = doubaoRequest(session, '/samantha/aispace/homepage', {})
  const folder = home.children?.find((node) => node.name === '我的创作')
  if (!folder?.id) throw new Error('download_pending: 豆包创作目录尚未就绪')
  let cursor
  for (let page = 0; page < 100; page++) {
    const data = doubaoRequest(session, '/samantha/aispace/node_info', {
      node_id: folder.id,
      need_full_path: true,
      size: 50,
      sort_param: { need_sort_config: true, sort_order: 1, sort_type: 0 },
      ...(cursor ? { cursor } : {}),
    })
    const node = data.children?.find((item) => String(item.key) === String(vid))
    if (node?.id) {
      const download = doubaoRequest(session, '/samantha/aispace/get_download_info', {
        requests: [{ node_id: node.id }],
      })
      const url = download.download_infos?.[0]?.main_url
      if (url && /^https:\/\//.test(url)) return url
      throw new Error('download_pending: 豆包无水印视频尚未就绪')
    }
    if (!data.has_more || !data.next_cursor || data.next_cursor === cursor) break
    cursor = data.next_cursor
  }
  throw new Error('download_pending: 本次生成视频尚未进入创作目录')
}

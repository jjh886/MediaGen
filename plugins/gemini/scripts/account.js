function geminiIdentity(data) {
  const email = typeof data?.oPEP7c === 'string' ? data.oPEP7c.trim() : ''
  const userId = String(data?.S06Grb || '')
  if (data?.Lz8rbb !== true || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email) || !/^\d+$/.test(userId))
    return null
  return { email, username: email, user_id: userId }
}

function geminiAccountProfile(identity, status) {
  const products = Array.isArray(status?.[16]) ? status[16] : []
  const features = Array.isArray(status?.[17]) ? status[17] : []
  const code = status?.[14]
  if (code === 1016) throw new Error('LOGIN_EXPIRED: Gemini 账号未登录')
  if (code != null && code !== 1000)
    throw new Error('AUTH_INCONCLUSIVE: Gemini 账号状态尚未就绪；code=' + code)
  if (!Array.isArray(status?.[15]) || !Array.isArray(status?.[16]))
    throw new Error('AUTH_INCONCLUSIVE: Gemini 账号资料不完整')
  const tier =
    products.includes(16) || features.includes(106)
      ? 'Google AI Ultra'
      : features.includes(198)
        ? 'Google AI Plus'
        : products.includes(8) || features.includes(19)
          ? 'Google AI Pro'
          : products.includes(21) || products.includes(22)
            ? 'Google Workspace'
            : 'Free'
  return { ...identity, tier }
}

function geminiQuotaWindows(modeQuota, quota, mediaQuota) {
  const windows = []
  if (typeof quota?.[0] === 'boolean') {
    const reset = Number(quota[2]?.[0])
    windows.push({
      name: quota[0] ? '文本额度（已达上限）' : '文本额度（未达上限，总量未公开）',
      task_kind: 'text',
      ...(quota[0] ? { remaining_count: 0, unit: '次' } : {}),
      ...(reset > 0 && Number.isFinite(reset)
        ? { reset_at: new Date(reset * 1000).toISOString() }
        : {}),
    })
  }
  for (const row of Array.isArray(modeQuota?.[0]) ? modeQuota[0] : []) {
    const names = { 1: 'Flash', 3: 'Pro', 6: 'Flash-Lite', 8: 'Argon' }
    const reset = Number(row?.[4]?.[0])
    windows.push({
      name: (names[row?.[0]] || '模型') + '（部分功能受限）',
      task_kind: 'text',
      ...(reset > 0 && Number.isFinite(reset)
        ? { reset_at: new Date(reset * 1000).toISOString() }
        : {}),
    })
  }
  for (const [action, name, kind] of [
    [7, '图片', 'image'],
    [5, '视频', 'video'],
  ]) {
    const row = (Array.isArray(mediaQuota?.[0]) ? mediaQuota[0] : []).find(
      (row) => row?.[0]?.[1] === action
    )
    const used = row?.[2]
    const reset = Number(row?.[3]?.[0])
    windows.push({
      name: name + (typeof used === 'number' ? '额度' : '额度（上游未提供数据）'),
      task_kind: kind,
      ...(typeof used === 'number' && Number.isFinite(used) && used >= 0 && used <= 100
        ? { remaining_percent: 100 - used }
        : {}),
      ...(reset > 0 && Number.isFinite(reset)
        ? { reset_at: new Date(reset * 1000).toISOString() }
        : {}),
    })
  }
  return windows
}

async function geminiBrowserAccount() {
  if (location.hostname !== 'gemini.google.com')
    return { logged_in: false, reason: '请先完成 Google 登录' }
  const data = window.WIZ_global_data || {}
  const identity = geminiIdentity(data)
  if (!identity) return { logged_in: false, reason: '尚未检测到完整的 Google 登录身份' }
  if (!data.SNlM0e || !data.cfb2h)
    return { logged_in: null, reason: '正在等待 Gemini 登录会话就绪' }
  const authuser = Number(location.pathname.match(/^\/u\/(\d+)\//)?.[1] || 0)
  const rpc = async (id, args = []) => {
    const url = new URL(
      (authuser ? '/u/' + authuser : '') + '/_/BardChatUi/data/batchexecute',
      location.origin
    )
    url.search = new URLSearchParams({
      rpcids: id,
      bl: data.cfb2h,
      hl: 'zh-CN',
      rt: 'c',
    }).toString()
    const response = await fetch(url, {
      method: 'POST',
      credentials: 'include',
      signal: AbortSignal.timeout(8000),
      body: new URLSearchParams({
        at: data.SNlM0e,
        'f.req': JSON.stringify([[[id, JSON.stringify(args), null, 'generic']]]),
      }),
    })
    if (!response.ok) throw new Error('Gemini 账号检查 HTTP ' + response.status)
    for (const line of (await response.text()).split('\n')) {
      if (!line.startsWith('[[')) continue
      for (const row of JSON.parse(line))
        if (row[0] === 'wrb.fr' && row[1] === id && typeof row[2] === 'string')
          return JSON.parse(row[2])
    }
    throw new Error('Gemini 账号检查未返回资料')
  }
  try {
    const account = geminiAccountProfile(identity, await rpc('otAQ7b'))
    try {
      const [mode, quota, media] = await Promise.all([
        rpc('VxUbXb'),
        rpc('aPya6c'),
        rpc('qpEbW', [
          [
            [1, 7],
            [1, 5],
          ],
        ]),
      ])
      return { ...account, logged_in: true, quota_windows: geminiQuotaWindows(mode, quota, media) }
    } catch (_) {
      return { ...account, logged_in: true, reason: '登录已确认，额度暂时无法读取' }
    }
  } catch (error) {
    return { logged_in: null, reason: String(error.message) }
  }
}

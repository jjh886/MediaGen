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

const GEMINI_PROFILE_ARGS = [
  ['me'],
  [[['person.photo', 'person.name', 'person.email']], null, [1, 7]],
]

function geminiAccountDetails(identity, data) {
  const person = data?.[0]?.find((entry) => entry?.[0] === 'me')?.[2]
  if (!person || String(person[0]) !== identity.user_id) return {}
  const name = person[2]?.[0]?.[1]
  const avatar = person[3]?.[0]?.[1]
  return {
    ...(typeof name === 'string' && name.trim() ? { username: name.trim() } : {}),
    ...(typeof avatar === 'string' &&
    /^https:\/\/lh[0-9]+\.googleusercontent\.com\/[^\s<>"']+$/.test(avatar)
      ? { avatar_url: avatar }
      : {}),
  }
}

function geminiUsagePath() {
  // 来自网页 UsagePageQuery；失败时保留旧额度，不再用工具枚举推测图片或视频额度。
  const variables = {
    g1Request: {
      entrypoints: [
        'GEMINI_USAGE_SETTINGS_UPGRADE_BANNER',
        'GEMINI_USAGE_SETTINGS_AI_CREDITS_UPSELL_CARD',
        'GEMINI_USAGE_SETTINGS_UPGRADE_AND_CREDITS_BANNER',
      ],
    },
    workspaceRequest: {
      recommendationsForSurfaceRequests: [
        {
          recommendationSurface: { rpOnramp: 'GEMINI_CHAT_USAGE_LIMITS_UPSELL', version: '1' },
          maxRecommendationCount: 1,
        },
      ],
    },
  }
  return (
    '/_/BardChatUi/graphql/schemas/GEMINI_WEB_GRAPHQL/executeQuery?query_signature=' +
    encodeURIComponent('2/r9A7h8PGYFROeibFhACp9h3mHf72UWS3JhgYaQYlPoo=') +
    '&variables=' +
    encodeURIComponent(JSON.stringify(variables))
  )
}

function geminiUsageWindows(raw) {
  let frames
  try {
    frames = JSON.parse(String(raw).replace(/^\s*\)\]\}'\s*/, ''))
  } catch (_) {
    throw new Error('AUTH_INCONCLUSIVE: Gemini 用量响应格式无效')
  }
  const usage = (Array.isArray(frames) ? frames : [])
    .map((frame) => frame?.data?.viewer?.rpcs?.getUsageInfo)
    .find((value) => Array.isArray(value?.pools))
  if (!usage) throw new Error('AUTH_INCONCLUSIVE: Gemini 未返回统一用量数据')
  return usage.pools.map((pool) => {
    const used = pool?.percentageUsed
    if (
      typeof pool?.poolType !== 'string' ||
      !pool.poolType ||
      typeof used !== 'number' ||
      !Number.isFinite(used) ||
      used < 0
    )
      throw new Error('AUTH_INCONCLUSIVE: Gemini 用量窗口数据不完整')
    const window = {
      name: { GXU_FIVE_HOURLY: '5 小时额度', GXU_WEEKLY: '周额度' }[pool.poolType] || pool.poolType,
      remaining_percent: Math.max(0, 100 - Math.round(used * 100)),
    }
    if (pool.gxu?.windowRefreshesAt) {
      const reset = Date.parse(pool.gxu.windowRefreshesAt)
      if (!Number.isFinite(reset)) throw new Error('AUTH_INCONCLUSIVE: Gemini 用量窗口恢复时间无效')
      window.reset_at = new Date(reset).toISOString()
    }
    return window
  })
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
    const account = geminiAccountProfile(identity, await rpc('otAQ7b', [1]))
    try {
      Object.assign(
        account,
        geminiAccountDetails(identity, await rpc('o30O0e', GEMINI_PROFILE_ARGS))
      )
    } catch (_) {
      /* 资料接口失败不覆盖已确认身份。 */
    }
    try {
      const response = await fetch((authuser ? '/u/' + authuser : '') + geminiUsagePath(), {
        credentials: 'include',
        signal: AbortSignal.timeout(8000),
        headers: { accept: 'application/json' },
      })
      if (!response.ok) throw new Error('Gemini 用量查询 HTTP ' + response.status)
      return {
        ...account,
        logged_in: true,
        quota_windows: geminiUsageWindows(await response.text()),
      }
    } catch (_) {
      return { ...account, logged_in: true, reason: '登录已确认，额度暂时无法读取' }
    }
  } catch (error) {
    return { logged_in: null, reason: String(error.message) }
  }
}

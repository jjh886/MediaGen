function deepseekInitializeSession(session) {
  for (const key of ['userId', 'settings', 'usedSettings', 'settingsRefreshAt'])
    Object.defineProperty(session, key, { value: undefined, writable: true })
  if (!session.auth.device_id) throw new Error('LOGIN_EXPIRED: 请重新登录以采集 DeepSeek 设备身份')
  if (!session.auth.device_checked) {
    deepseekRequest(
      session,
      '/users/auth_token/check_device',
      {
        device_id: session.auth.device_id,
        device_model: '',
      },
      'POST',
      10000
    )
    session.auth.device_checked = true
    host.call('file.write_json', { path: session.auth_path, value: session })
  }
  const user = deepseekRequest(session, '/users/current', undefined, 'GET')
  if (!user.id) throw new Error('AUTH_INCONCLUSIVE: DeepSeek 身份响应不完整')
  session.userId = user.id
  deepseekLoadSettings(session)
  deepseekRefreshHif(session)
}

function deepseekLoadSettings(session) {
  const used = session.usedSettings || new Set()
  try {
    const settings = deepseekRequest(
      session,
      '/client/settings?did=' + encodeURIComponent(session.auth.device_id) + '&scope=main',
      undefined,
      'GET'
    )
    if (
      !settings.settings ||
      typeof settings.settings !== 'object' ||
      Array.isArray(settings.settings)
    )
      throw new Error('invalid_response: DeepSeek 客户端配置无效')
    session.settings = settings.settings
    session.auth.client_settings = settings.settings
    host.call('file.write_json', { path: session.auth_path, value: session })
  } catch (error) {
    if (String(error?.message || '').startsWith('LOGIN_EXPIRED:')) throw error
    session.settings ||= session.auth.client_settings || {}
    session.settingsRefreshAt = Date.now() + 60000
  }
  session.usedSettings = used
}

function deepseekSetting(session, key, fallback) {
  const entry = session.settings?.[key]
  if (!entry || !Number.isInteger(entry.id) || !Number.isFinite(entry.value) || entry.value <= 0)
    return fallback
  session.usedSettings.add(entry.id)
  return entry.value
}

function deepseekReportSettings(session) {
  if (!session.usedSettings?.size) return
  try {
    deepseekRequest(session, '/client/settings/report', {
      settings_ids: [...session.usedSettings],
      did: session.auth.device_id,
      sso_id: session.userId,
    })
    session.usedSettings.clear()
  } catch (error) {
    if (String(error?.message || '').startsWith('LOGIN_EXPIRED:')) throw error
  }
}

function deepseekRefreshHif(session) {
  session.auth.hif ||= {}
  const maxBackoff = deepseekSetting(session, 'hif_max_retry_interval_secs', 600) * 1000
  for (const key of ['leim', 'dliq']) {
    const cached = session.auth.hif[key] || {}
    if (Number(cached.refresh_at) > Date.now()) continue
    const origin = 'https://hif-' + key + '.deepseek.com'
    const headers = deepseekHeaders(session, '/api/v0/', 'GET')
    for (const name of [
      'authorization',
      'cookie',
      'x-device-id',
      'x-device-model',
      'x-settings-token',
      'x-hif-leim',
      'x-hif-dliq',
    ])
      delete headers[name]
    headers.referer = DEEPSEEK_ORIGIN + '/'
    headers.origin = DEEPSEEK_ORIGIN
    headers['sec-fetch-site'] = 'same-site'
    try {
      apiSessionRequireOk(
        host.call('http.request', {
          method: 'OPTIONS',
          url: origin + '/query',
          headers: {
            accept: '*/*',
            origin: DEEPSEEK_ORIGIN,
            'user-agent': headers['user-agent'],
            'sec-fetch-mode': 'cors',
            'access-control-request-method': 'GET',
            'access-control-request-headers':
              'x-client-bundle-id,x-client-locale,x-client-platform,x-client-timezone-offset,x-client-version',
          },
          proxy: 'system',
          timeout_ms: 3000,
          retry: { max_attempts: 1 },
        }),
        'DeepSeek HIF 预检失败'
      )
      const response = apiSessionRequireOk(
        host.call('http.request', {
          method: 'GET',
          url: origin + '/query',
          headers,
          proxy: 'system',
          timeout_ms: 3000,
          retry: { max_attempts: 1 },
        }),
        'DeepSeek HIF 请求失败'
      )
      const data = response.body?.data
      if (
        response.body?.code !== 0 ||
        data?.biz_code !== 0 ||
        typeof data?.biz_data?.value !== 'string' ||
        !data.biz_data.value ||
        data.biz_data.value.length > 16384
      )
        throw new Error('invalid_response: DeepSeek HIF 响应无效')
      const ttl = Number(response.headers?.['x-hif-ttl'])
      session.auth.hif[key] = {
        value: data.biz_data.value,
        refresh_at: Date.now() + (Number.isFinite(ttl) && ttl > 0 ? ttl : 600) * 1000,
      }
    } catch (_) {
      // 网页保留上次成功值，并按退避时间重试；辅助校验服务故障不能使账号退出登录。
      const delay = Math.min(maxBackoff, (Number(cached.retry_delay) || 500) * 2)
      session.auth.hif[key] = { ...cached, refresh_at: Date.now() + delay, retry_delay: delay }
    }
  }
  host.call('file.write_json', { path: session.auth_path, value: session })
}

function deepseekSessionTick(session) {
  if (Date.now() >= session.settingsRefreshAt) deepseekLoadSettings(session)
  if (Object.values(session.auth.hif || {}).some((entry) => Date.now() >= entry.refresh_at))
    deepseekRefreshHif(session)
}

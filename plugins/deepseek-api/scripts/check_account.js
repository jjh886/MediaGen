function main(ctx) {
  try {
    const data = deepseekRequest(apiSessionRead(ctx), '/users/current', undefined, 'GET')
    if (!data.id) throw new Error('AUTH_INCONCLUSIVE: DeepSeek 身份响应不完整')
    return {
      logged_in: true,
      status: data.chat?.is_muted ? 'cooldown' : 'available',
      user_id: String(data.id),
      username: data.name || data.mobile_number || data.email || '',
      name: data.name || data.mobile_number || data.email || '',
      email: data.mobile_number || data.email || '',
      reason: data.chat?.is_muted
        ? '平台暂时限制此账号生成，请稍后刷新'
        : '登录有效；平台未提供固定剩余额度',
    }
  } catch (error) {
    return apiSessionHealthError(error)
  }
}

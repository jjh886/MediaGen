function main(ctx) {
  try {
    return chatgptApiAvailableAccount(chatgptApiRefreshSession(chatgptApiReadAuth(ctx)))
  } catch (error) {
    return chatgptApiAccountCheckError(error)
  }
}

function chatgptApiAvailableAccount(auth) {
  const account = auth.auth.account || {}
  const user = auth.auth.user || {}
  return {
    logged_in: true,
    status: 'available',
    reason: 'ChatGPT API 登录有效',
    account: {
      user_id: user.id || account.id || '',
      username: user.name || user.email || account.id || 'ChatGPT API',
      email: user.email || '',
      name: user.name || user.email || 'ChatGPT API',
    },
  }
}

function chatgptApiAccountCheckError(error) {
  const message = String(error?.message || error || 'ChatGPT API 登录检查失败')
  const inconclusive = message.startsWith('AUTH_INCONCLUSIVE:')
  return {
    logged_in: inconclusive ? null : false,
    status: inconclusive ? 'inconclusive' : 'login_expired',
    reason: message.replace(/^(AUTH_INCONCLUSIVE|LOGIN_EXPIRED):\s*/, ''),
  }
}

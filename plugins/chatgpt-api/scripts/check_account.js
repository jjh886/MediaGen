function main(ctx) {
  try {
    return chatgptApiAvailableAccount(chatgptApiRefreshSession(chatgptApiReadAuth(ctx)))
  } catch (error) {
    return chatgptApiAccountCheckError(error)
  }
}

function chatgptApiAvailableAccount(auth) {
  const quota = chatgptApiImageQuotaStatus(auth)
  return {
    logged_in: true,
    ...chatgptApiQuotaFields(quota),
    account: chatgptApiAccountIdentity(auth),
  }
}

function chatgptApiQuotaFields(quota) {
  return {
    status: chatgptApiFirstText([quota?.status, 'available']),
    reason: chatgptApiFirstText([quota?.reason, 'ChatGPT API 登录有效']),
    cooldown_seconds: quota?.cooldown_seconds || null,
  }
}

function chatgptApiAccountIdentity(auth) {
  const account = auth.auth.account || {}
  const user = auth.auth.user || {}
  const displayName = chatgptApiFirstText([user.name, user.email, account.id, 'ChatGPT API'])
  return {
    user_id: chatgptApiFirstText([user.id, account.id]),
    username: displayName,
    email: chatgptApiFirstText([user.email]),
    name: displayName,
  }
}

function chatgptApiFirstText(values) {
  return String((values || []).find((value) => String(value || '').trim()) || '').trim()
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

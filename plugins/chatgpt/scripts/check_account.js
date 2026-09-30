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
    reason: chatgptApiFirstText([quota?.reason, 'ChatGPT 登录有效']),
    cooldown_seconds: quota?.cooldown_seconds || null,
    quota_task_kind: quota?.quota_task_kind || null,
  }
}

function chatgptApiAccountIdentity(auth) {
  const account = auth.auth.account || {}
  const user = auth.auth.user || {}
  const displayName = chatgptApiFirstText([user.name, user.email, account.id, 'ChatGPT'])
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
  const message = String(error?.message || error || 'ChatGPT 登录检查失败')
  const expired = message.startsWith('LOGIN_EXPIRED:')
  const verification = message.startsWith('verification_required:')
  return {
    logged_in: expired ? false : null,
    status: verification ? 'verification_required' : expired ? 'login_expired' : 'inconclusive',
    reason: message.replace(/^(AUTH_INCONCLUSIVE|LOGIN_EXPIRED|verification_required):\s*/, ''),
  }
}

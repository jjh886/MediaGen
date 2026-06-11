function main(ctx) {
  let auth
  try {
    auth = codexEnsureFreshAuth(ctx)
  } catch (error) {
    return codexHealthFromError(error)
  }

  let result = codexFetchAccountProfile(auth)
  if (!result.success && codexHttpStatus(result) === 401) {
    try {
      auth = codexRefreshAuth(ctx, auth, '账号检查返回 401')
      result = codexFetchAccountProfile(auth)
    } catch (error) {
      return codexHealthFromError(error)
    }
  }

  return codexHealthFromHttpResult(result, auth)
}

function codexHealthFromError(error) {
  const message = String((error && error.message) || error || 'Codex 登录检查失败')
  if (message.startsWith('AUTH_INCONCLUSIVE:')) {
    return {
      logged_in: null,
      status: 'inconclusive',
      reason: message.replace(/^AUTH_INCONCLUSIVE:\s*/, ''),
    }
  }
  return {
    logged_in: false,
    status: 'login_expired',
    reason: message.replace(/^LOGIN_EXPIRED:\s*/, ''),
  }
}

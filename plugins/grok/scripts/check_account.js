function main(ctx) {
  const auth = grokAuth(ctx)
  grokRequest(ctx, auth, '/models', undefined, { build: true })
  return {
    loggedIn: true,
    userId: auth.user_id,
    email: auth.email || '',
    username: auth.email || auth.user_id,
    tier: 'free',
  }
}

;(async () => {
  const ctx = window[Symbol.for('mediagen.doubao.api')] || {}
  return doubaoInternalGenerateVideo(ctx, doubaoAppConfig())
})()

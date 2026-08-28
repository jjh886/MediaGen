;(async () => {
  const ctx = window[Symbol.for('mediagen.doubao.api')] || {}
  return doubaoInternalGenerateImage(ctx, doubaoAppConfig())
})()

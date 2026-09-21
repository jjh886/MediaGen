function main(ctx) {
  return { success: true, response: grokText(ctx, grokAuth(ctx), ctx.request) }
}

function main(ctx) {
  const result = grokRequest(ctx, grokAuth(ctx), '/models', undefined, { build: true })
  if (!Array.isArray(result.body?.data)) throw new Error('Grok 模型目录格式无效')
  return {
    models: result.body.data
      .filter(
        (item) =>
          typeof item?.id === 'string' &&
          item.id.trim() &&
          !/imagine|voice|transcri|tts|stt/i.test(item.id)
      )
      .map((item) => ({
        model_id: item.id.trim(),
        display_name: item.name || item.id.trim(),
        task_kind: 'text',
        capabilities: grokTextCapabilities(item.id.trim(), item),
      })),
  }
}

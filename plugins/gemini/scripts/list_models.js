function main(ctx) {
  const session = geminiInitialize(apiSessionRead(ctx))
  return {
    models: geminiCatalog(session).map((model) => ({
      model_id: model.id,
      display_name: model.name,
      task_kind: 'text',
      capabilities: {
        input_modalities: ['text', 'image', 'video', 'audio', 'file'],
        output_modalities: ['text'],
        tool_calling: false,
        streaming: true,
        reasoning: { efforts: model.efforts, default_effort: model.efforts[0] || '' },
      },
    })),
  }
}

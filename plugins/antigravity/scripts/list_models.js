function main(ctx) {
  const { models } = agCatalog(ctx, agAuth(ctx))
  return {
    models: AG_MODELS.flatMap((slug) => {
      const variants = agVariants(slug, models)
      const levels = Object.keys(variants)
      if (!levels.length) return []
      const efforts = variants.fixed ? [] : levels
      const preferred = slug.startsWith('claude-') ? 'high' : 'medium'
      const defaultEffort = efforts.includes(preferred) ? preferred : efforts[0]
      const display = slug
        .replace('-5-5', '-5.5')
        .split('-')
        .map((part) => part[0].toUpperCase() + part.slice(1))
        .join(' ')
      const base = {
        model_id: 'antigravity-' + slug,
        display_name: display,
        task_kind: 'text',
        capabilities: {
          ...agInputCapabilities(models, variants),
          output_modalities: ['text'],
          endpoints: ['/v1/responses', '/v1/chat/completions'],
          streaming: true,
          tool_calling: false,
          unsupported_parameters: [
            'tools',
            'temperature',
            'top_p',
            'stop',
            'text',
            ...(slug.startsWith('claude-') ? [] : ['max_output_tokens']),
          ],
          reasoning: {
            efforts,
            ...(defaultEffort
              ? {
                  default_effort: defaultEffort,
                  model_ids: Object.fromEntries(
                    efforts.map((e) => [e, 'antigravity-' + slug + '-' + e])
                  ),
                }
              : {}),
          },
        },
      }
      return [
        base,
        ...efforts.map((level) => ({
          ...base,
          model_id: base.model_id + '-' + level,
          aliases: [variants[level], base.model_id + '(' + level + ')'],
          catalog_hidden: true,
          capabilities: {
            ...base.capabilities,
            ...agInputCapabilities(models, { [level]: variants[level] }),
            reasoning: { efforts: [level], default_effort: level },
          },
        })),
      ]
    }),
  }
}

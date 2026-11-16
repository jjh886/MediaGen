// 目录同步与对话使用同一份网页预设，升级提示不能当作账号已经拥有的档位。
const CHATGPT_MODELS_PATH = '/backend-api/models?iim=false&include_icons=false&is_gizmo=false'
const CHATGPT_CHAT_MODELS = [
  { id: 'chatgpt-gpt-6', title: 'GPT-6', base: 'gpt-6' },
  { id: 'chatgpt-gpt-5.6-sol', title: 'GPT-5.6 Sol', base: 'gpt-5-6' },
]

function chatgptApiPickerOptions(catalog, family) {
  const models = catalog.models
  const has = (slug) => models.some((m) => m?.slug === slug)
  // latest 的含义会变，按预设实际指向的型号匹配家族。
  const routes = new Map()
  for (const version of Array.isArray(catalog.versions) ? catalog.versions : []) {
    if (version?.enabled === false || !Array.isArray(version?.intelligence_presets)) continue
    for (const preset of version.intelligence_presets || []) {
      if (
        preset?.preset_type !== 'available' ||
        typeof preset.model_slug !== 'string' ||
        !has(preset.model_slug)
      )
        continue
      const { model_slug: model, thinking_effort } = preset
      if (model !== family.base && !model.startsWith(family.base + '-')) continue
      if (preset.lane === 'instant') routes.set('none', { model })
      if (preset.lane === 'pro') routes.set('pro', { model, thinking_effort })
      if (preset.lane === 'thinking' && ['standard', 'extended', 'max'].includes(thinking_effort))
        routes.set(thinking_effort, { model, thinking_effort })
    }
  }
  // 旧目录可能只给模型列表，仍按账号实际列出的型号和能力路由。
  if (!Array.isArray(catalog.versions)) {
    const instant = family.base + '-instant'
    const thinking = models.find((m) => m?.slug === family.base + '-thinking')
    if (has(instant)) routes.set('none', { model: instant })
    for (const item of thinking?.thinking_efforts || []) {
      const effort = typeof item === 'string' ? item : item?.thinking_effort
      if (['standard', 'extended', 'max'].includes(effort))
        routes.set(effort, { model: thinking.slug, thinking_effort: effort })
    }
    if (has(family.base + '-pro')) routes.set('pro', { model: family.base + '-pro' })
  }
  return routes
}

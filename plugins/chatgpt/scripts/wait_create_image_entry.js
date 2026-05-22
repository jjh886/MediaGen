;(() => {
  const visible = (el) => {
    if (!el) return false
    const rect = el.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle(el)
    if (!style) return true
    if (style.display === 'none') return false
    if (style.visibility === 'hidden') return false
    if (style.pointerEvents === 'none') return false
    return Number(style.opacity || '1') !== 0
  }

  const normalize = (text) =>
    String(text || '')
      .replace(/\s+/g, ' ')
      .trim()
  const labels = ['创建图片', 'Create image']
  const start = Date.now()

  return new Promise((resolve) => {
    const timer = setInterval(() => {
      const menus = Array.from(document.querySelectorAll('[role="menu"]')).filter(visible)
      for (const menu of menus) {
        const option = Array.from(
          menu.querySelectorAll(
            '[role="menuitem"], [role="menuitemradio"], button, [role="option"]'
          )
        ).find((el) => {
          if (!visible(el)) return false
          const text = normalize(el.textContent)
          return labels.some(
            (label) => text === label || text.startsWith(label) || text.includes(label)
          )
        })
        if (option) {
          clearInterval(timer)
          const text = normalize(option.textContent)
          const target = labels.find((label) => text.includes(label)) || '创建图片'
          resolve(JSON.stringify({ found: true, target_text: target, w: 1 }))
          return
        }
      }

      if (Date.now() - start >= 5000) {
        clearInterval(timer)
        resolve(JSON.stringify({ error: '未进入图片模式，且未找到创建图片入口' }))
      }
    }, 100)
  })
})()

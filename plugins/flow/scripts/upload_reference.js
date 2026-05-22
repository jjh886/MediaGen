// Flow: 选择已上传的参考图作为生成参考
// 前置条件：引擎已通过 browser.set_file 将图片注入到 input[type=file]，
//           Flow 前端自动调 uploadImage API 完成上传，图片已出现在项目列表中。
// 本脚本负责：点击 + (add_2) 按钮 → 在弹出的 dialog 中选择最新的参考图
;(async () => {
  function sleep(ms) {
    return new Promise((r) => setTimeout(r, ms))
  }

  try {
    // 检查是否有已上传的图片（如果项目为空或只有提示文字，则没有参考图需要选择）
    const fileInput = document.querySelector('input[type="file"][accept*="image"]')
    if (!fileInput || !fileInput.files?.length) {
      // 没有通过 set_file 设置的文件，跳过
      return JSON.stringify({ skipped: true, reason: '无参考图文件' })
    }

    // 等待图片出现在项目列表中（可能还在上传）
    let uploadReady = false
    for (let i = 0; i < 15; i++) {
      const imgs = document.querySelectorAll('img[src*="getMediaUrlRedirect"]')
      if (imgs.length > 0) {
        uploadReady = true
        break
      }
      await sleep(1000)
    }

    if (!uploadReady) {
      return JSON.stringify({ skipped: true, reason: '参考图未出现在项目列表' })
    }

    // 点击 + 按钮打开参考图选择 dialog
    const addBtn = document.querySelector('button[aria-haspopup="dialog"]')
    if (!addBtn) {
      return JSON.stringify({ error: '未找到 + (add_2) 按钮' })
    }

    addBtn.click()
    await sleep(1500)

    // 等待 dialog 出现
    let dialog = document.querySelector('[role="dialog"]')
    if (!dialog) {
      // 重试
      addBtn.click()
      await sleep(2000)
      dialog = document.querySelector('[role="dialog"]')
    }
    if (!dialog) {
      return JSON.stringify({ error: '参考图选择 dialog 未弹出' })
    }

    // 在 dialog 中找到图片列表，点击第一个（最新上传的在前面）
    const dialogImgs = dialog.querySelectorAll('img[src*="getMediaUrlRedirect"]')
    if (dialogImgs.length === 0) {
      // dialog 可能还在加载
      await sleep(2000)
      const retryImgs = dialog.querySelectorAll('img[src*="getMediaUrlRedirect"]')
      if (retryImgs.length === 0) {
        return JSON.stringify({ error: 'dialog 中无可选图片' })
      }
    }

    // 点击第一张图片（最新）
    const targetImg = dialog.querySelectorAll('img[src*="getMediaUrlRedirect"]')[0]
    if (!targetImg) {
      return JSON.stringify({ error: '无法找到参考图' })
    }

    // 点击图片或其父容器
    const clickTarget = targetImg.closest('div[tabindex]') || targetImg.parentElement
    if (clickTarget) {
      clickTarget.click()
    } else {
      targetImg.click()
    }
    await sleep(1000)

    // 验证：输入框区域应该出现参考图缩略图
    const inputArea = document.querySelector('div[role="textbox"]')?.parentElement?.parentElement
    const refThumb = inputArea?.querySelector('img[src*="getMediaUrlRedirect"]')

    return JSON.stringify({
      selected: true,
      refVisible: !!refThumb,
    })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()

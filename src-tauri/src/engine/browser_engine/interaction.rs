use std::sync::Arc;
use std::time::Duration;

use headless_chrome::browser::tab::point::Point;
use headless_chrome::protocol::cdp::Input;
use headless_chrome::Tab;
use serde_json::json;
use tracing::debug;

use crate::error::AppError;
use crate::types::StepResult;

/// 基于 SystemTime 纳秒的伪随机，避免引入 rand 依赖。返回 [min, max)。
fn rand_f64(min: f64, max: f64) -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as f64;
    let frac = (nanos / 1_000_000_000.0).fract();
    min + (max - min) * frac
}

fn rand_range_u64(min_ms: u64, max_ms: u64) -> u64 {
    if max_ms <= min_ms {
        return min_ms;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    min_ms + nanos % (max_ms - min_ms + 1)
}

/// 类真人鼠标点击：先从 target 附近一个随机起点 mouseMoved 过来，再 click。
/// 落点带 ±4 像素随机偏移，避免永远点在正中心。
fn human_click_point(tab: &Arc<Tab>, target_x: f64, target_y: f64) -> Result<(), AppError> {
    let offset_x = rand_f64(-4.0, 4.0);
    let offset_y = rand_f64(-4.0, 4.0);
    let click_x = target_x + offset_x;
    let click_y = target_y + offset_y;

    // 起点：目标附近的随机位置（距离 100-250 像素）
    let start_x = target_x + rand_f64(-250.0, 250.0);
    let start_y = target_y + rand_f64(-200.0, 200.0);

    let steps = 5 + (rand_range_u64(0, 5) as usize);
    for i in 1..=steps {
        let progress = i as f64 / steps as f64;
        // easeInOut（三次方）让轨迹有加减速
        let eased = if progress < 0.5 {
            4.0 * progress.powi(3)
        } else {
            1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0
        };
        // 加一点垂直扰动
        let jitter_x = rand_f64(-1.5, 1.5);
        let jitter_y = rand_f64(-1.5, 1.5);
        let x = start_x + (click_x - start_x) * eased + jitter_x;
        let y = start_y + (click_y - start_y) * eased + jitter_y;

        tab.call_method(Input::DispatchMouseEvent {
            Type: Input::DispatchMouseEventTypeOption::MouseMoved,
            x,
            y,
            modifiers: None,
            timestamp: None,
            button: None,
            buttons: None,
            click_count: None,
            force: None,
            tangential_pressure: None,
            tilt_x: None,
            tilt_y: None,
            twist: None,
            delta_x: None,
            delta_y: None,
            pointer_Type: None,
        })
        .map_err(|e| AppError::BrowserError(format!("mouseMoved 失败: {e}")))?;
        std::thread::sleep(Duration::from_millis(rand_range_u64(15, 45)));
    }

    // 短暂停顿再点击（真人点击前的"瞄准"）
    std::thread::sleep(Duration::from_millis(rand_range_u64(30, 90)));

    tab.click_point(Point {
        x: click_x,
        y: click_y,
    })
    .map_err(|e| AppError::BrowserError(format!("CDP 点击失败: {e}")))?;
    Ok(())
}

/// 每次动作前随机 500-1500ms 延迟（阻塞等待）
fn human_pre_delay() {
    std::thread::sleep(Duration::from_millis(rand_range_u64(500, 1500)));
}

impl super::BrowserEngine {
    /// browser.click: 通过 CSS 选择器查找元素并点击（CDP 鼠标事件）
    ///
    /// 为对抗行为检测：
    /// - 动作前 500-1500ms 随机延迟
    /// - 鼠标从目标附近随机起点 mouseMoved 走 5-10 步到目标（带 easing + 抖动）
    /// - 落点带 ±4 像素随机偏移，避免永远点击元素正中心
    pub(crate) async fn click_element(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.click 缺少 session_id".into()))?;
        let selector = params["selector"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.click 缺少 selector".into()))?;
        let wait_ms = params["wait_ms"].as_u64().unwrap_or(3000);

        let tab = self.get_tab(session_id)?;
        let selector = selector.to_string();

        let result = tokio::task::spawn_blocking(move || {
            human_pre_delay();

            let timeout = std::time::Duration::from_millis(wait_ms);
            let el = tab
                .wait_for_element_with_custom_timeout(&selector, timeout)
                .map_err(|e| AppError::BrowserError(format!("找不到元素 '{selector}': {e}")))?;

            // 通过 getBoundingClientRect 拿元素中心点（el.click 内部是 scrollIntoView + 中心点，
            // 但我们自己算中心点才能加轨迹和偏移）
            let script = format!(
                "(() => {{ const el = document.querySelector('{}'); if (!el) return null; \
                  el.scrollIntoView({{block:'center',behavior:'instant'}}); \
                  const r = el.getBoundingClientRect(); \
                  return JSON.stringify({{ x: r.x + r.width/2, y: r.y + r.height/2 }}); }})()",
                selector.replace('\\', "\\\\").replace('\'', "\\'")
            );
            let pos = tab
                .evaluate(&script, false)
                .map_err(|e| AppError::BrowserError(format!("取元素位置失败: {e}")))?;
            let point: serde_json::Value = pos
                .value
                .as_ref()
                .and_then(|v| v.as_str())
                .and_then(|s| serde_json::from_str(s).ok())
                .ok_or_else(|| AppError::BrowserError(format!("元素 '{selector}' 不可定位")))?;
            let x = point["x"].as_f64().unwrap_or(0.0);
            let y = point["y"].as_f64().unwrap_or(0.0);
            if x == 0.0 && y == 0.0 {
                // fallback：直接用 el.click（至少能工作）
                el.click()
                    .map_err(|e| AppError::BrowserError(format!("点击失败: {e}")))?;
                return Ok::<(), AppError>(());
            }
            human_click_point(&tab, x, y)?;
            Ok::<(), AppError>(())
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("click 任务失败: {e}")))?;

        result?;

        debug!(
            selector = params["selector"].as_str().unwrap_or(""),
            "元素已点击"
        );

        Ok(StepResult {
            success: true,
            data: json!({ "clicked": true }),
            error: None,
        })
    }

    /// browser.click_text: 通过文本内容查找按钮/元素并用 CDP 鼠标点击
    ///
    /// 用于需要真实鼠标事件的 popover 触发（JS click 无法打开时可用）
    pub(crate) async fn click_text(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.click_text 缺少 session_id".into()))?;
        let text = params["text"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.click_text 缺少 text".into()))?
            .to_string();
        let exact = params["exact"].as_bool().unwrap_or(false);
        let tag = params["tag"].as_str().unwrap_or("button").to_string();

        let tab = self.get_tab(session_id)?;

        let result = tokio::task::spawn_blocking(move || {
            human_pre_delay();

            // 通过 JS 找到元素位置（优先搜索 popover/dropdown 内的元素）
            let script = format!(
                r#"(() => {{
                    const searchText = '{}';
                    const isExact = {};
                    const tag = '{}';

                    // 两阶段匹配：
                    //   strict: startsWith/=== —— 命中"文本恰好是目标"的叶子/小节点
                    //   loose : includes        —— 兜底，但限制文本占比避免命中大容器
                    // 大容器（如 #root/body 内层）的 textContent 会把深层所有文字串起来，
                    // 若只用 includes，遍历按文档序先命中外层，CDP 就会点到视口正中央。
                    const matchStrict = (el) => {{
                        const t = (el.textContent || '').trim();
                        if (isExact) return t === searchText;
                        return t === searchText || t.startsWith(searchText);
                    }};
                    const matchLoose = (el) => {{
                        const t = (el.textContent || '').trim();
                        if (isExact) return t === searchText;
                        if (!t.includes(searchText)) return false;
                        // 文本占比保护：searchText 在 textContent 里必须占据较大比例，
                        // 否则说明当前节点是包含多段文字的容器，不是目标
                        return t.length <= searchText.length * 4;
                    }};

                    const getCenter = (el) => {{
                        const rect = el.getBoundingClientRect();
                        return {{ x: rect.x + rect.width / 2, y: rect.y + rect.height / 2, w: rect.width, h: rect.height }};
                    }};

                    // popover/dropdown/menu 选择器（最近弹出的浮层优先）
                    const popoverSelectors = '[class*="popover"]:not([class*="exited"]), [class*="dropdown"], [role="menu"], [role="listbox"], [class*="popup"]';
                    const sel = tag === '*' ? '*' : tag;

                    const findIn = (root, matcher, source) => {{
                        const els = root.querySelectorAll(sel);
                        for (const el of els) {{
                            if (!matcher(el) || el.offsetParent === null) continue;
                            const c = getCenter(el);
                            if (c.w > 0 && c.h > 0) return {{ found: true, ...c, source }};
                        }}
                        return null;
                    }};

                    const searchPopovers = (matcher) => {{
                        for (const pop of document.querySelectorAll(popoverSelectors)) {{
                            if (pop.offsetParent === null && !pop.style.display) continue;
                            const hit = findIn(pop, matcher, 'popover');
                            if (hit) return hit;
                        }}
                        return null;
                    }};

                    const search = (m) => searchPopovers(m) || findIn(document, m, 'page');

                    const hit = search(matchStrict) || search(matchLoose);
                    if (hit) return JSON.stringify(hit);
                    return JSON.stringify({{ found: false }});
                }})()"#,
                text.replace('\\', "\\\\").replace('\'', "\\'").replace('"', "\\\""),
                if exact { "true" } else { "false" },
                tag.replace('\\', "\\\\").replace('\'', "\\'").replace('"', "\\\""),
            );

            let result = tab.evaluate(&script, false)
                .map_err(|e| AppError::BrowserError(format!("查找元素失败: {e}")))?;

            let val: serde_json::Value = result.value
                .as_ref()
                .and_then(|v| v.as_str().and_then(|s| serde_json::from_str(s).ok()))
                .unwrap_or(serde_json::json!({"found": false}));

            if val["found"].as_bool() != Some(true) {
                return Err(AppError::BrowserError(format!("未找到文本为 '{}' 的元素", text)));
            }

            let x = val["x"].as_f64().unwrap_or(0.0);
            let y = val["y"].as_f64().unwrap_or(0.0);

            human_click_point(&tab, x, y)?;

            Ok::<(), AppError>(())
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("click_text 任务失败: {e}")))?;

        result?;
        debug!("click_text 完成");

        Ok(StepResult {
            success: true,
            data: json!({ "clicked": true }),
            error: None,
        })
    }

    /// browser.type_text: 通过 CSS 选择器查找元素，聚焦后用 CDP 键盘事件逐字输入
    ///
    /// 关键能力：发送的是可信（trusted）键盘事件，Slate.js / ProseMirror 等富文本编辑器
    /// 只接受可信的 InputEvent，普通 JS execCommand / dispatchEvent 无法触发。
    pub(crate) async fn type_text(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.type_text 缺少 session_id".into()))?;
        let selector = params["selector"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.type_text 缺少 selector".into()))?;
        let text = params["text"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.type_text 缺少 text".into()))?;
        let clear_first = params["clear_first"].as_bool().unwrap_or(false);
        let wait_ms = params["wait_ms"].as_u64().unwrap_or(3000);

        let tab = self.get_tab(session_id)?;
        let selector = selector.to_string();
        let text = text.to_string();

        let result = tokio::task::spawn_blocking(move || {
            human_pre_delay();

            let timeout = std::time::Duration::from_millis(wait_ms);
            let el = tab
                .wait_for_element_with_custom_timeout(&selector, timeout)
                .map_err(|e| AppError::BrowserError(format!("找不到元素 '{selector}': {e}")))?;

            // 先用 JS focus（不受 popover 遮挡影响），再用 CDP click
            let _ = tab.evaluate(
                &format!(
                    "document.querySelector('{}')?.focus()",
                    selector.replace('\'', "\\'")
                ),
                false,
            );
            std::thread::sleep(std::time::Duration::from_millis(rand_range_u64(80, 200)));

            el.click()
                .map_err(|e| AppError::BrowserError(format!("聚焦元素失败: {e}")))?;

            // 可选：清空已有内容（JS selectAll + Backspace）
            if clear_first {
                std::thread::sleep(std::time::Duration::from_millis(rand_range_u64(80, 180)));
                // 用 JS 全选（平台无关，比 Ctrl/Cmd+A 更可靠）
                let _ = tab.evaluate("document.execCommand('selectAll')", false);
                std::thread::sleep(std::time::Duration::from_millis(rand_range_u64(40, 90)));
                tab.press_key("Backspace")
                    .map_err(|e| AppError::BrowserError(format!("Backspace 失败: {e}")))?;
                std::thread::sleep(std::time::Duration::from_millis(rand_range_u64(80, 160)));
            }

            // 人类通常复制粘贴：CDP Input.insertText 一次性插入整段文本（会触发 beforeinput/input
            // 事件链，富文本编辑器如 Slate 可正常响应）
            // 注意：\n 在 Slate.js 中会触发换行或发送，替换为空格
            let clean_text = text.replace('\n', " ").replace('\r', " ");
            tab.send_character(&clean_text)
                .map_err(|e| AppError::BrowserError(format!("输入文本失败: {e}")))?;

            Ok::<(), AppError>(())
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("type_text 任务失败: {e}")))?;

        result?;

        debug!("文本已输入");

        Ok(StepResult {
            success: true,
            data: json!({ "typed": true }),
            error: None,
        })
    }

    /// browser.press_key: 发送特殊按键（Enter、Tab、Escape 等）
    pub(crate) async fn press_key(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.press_key 缺少 session_id".into()))?;
        let key = params["key"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.press_key 缺少 key".into()))?;

        let tab = self.get_tab(session_id)?;
        let key = key.to_string();

        let result = tokio::task::spawn_blocking(move || {
            human_pre_delay();
            tab.press_key(&key)
                .map_err(|e| AppError::BrowserError(format!("按键 '{key}' 失败: {e}")))?;
            Ok::<(), AppError>(())
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("press_key 任务失败: {e}")))?;

        result?;

        debug!(key = params["key"].as_str().unwrap_or(""), "按键已发送");

        Ok(StepResult {
            success: true,
            data: json!({ "pressed": true }),
            error: None,
        })
    }
}

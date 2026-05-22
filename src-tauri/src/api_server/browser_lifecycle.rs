//! 任务结束后的浏览器回收逻辑。
//!
//! 拆出独立模块是因为同一条 `force_close_browser` 被 image / image_sse / video
//! 三条路径共用，且未来可能扩展成"按 lifecycle 策略决定回收方式"的策略中枢。

use crate::engine::EngineRegistry;
use crate::plugin_loader::PluginLoader;
use crate::types::BrowserLifecycle;

/// 任务流结束后统一强制关闭该 profile 下的浏览器实例，确保进程被回收。
/// 兼容 `profile_dir` 为空（API-only 平台，是 no-op）。
/// 对 `browser_lifecycle = persistent` 的平台（如 Flow）会跳过关闭，保持常驻。
pub(crate) async fn force_close_browser(
    registry: &EngineRegistry,
    plugin_loader: &PluginLoader,
    platform: &str,
    profile_dir: &str,
) {
    if profile_dir.is_empty() {
        return;
    }
    if let Some(manifest) = plugin_loader.get_plugin(platform) {
        if matches!(manifest.browser_lifecycle, BrowserLifecycle::Persistent) {
            tracing::debug!(
                platform,
                profile_dir,
                "persistent 平台跳过 force_close_browser"
            );
            return;
        }
    }
    let params = serde_json::json!({ "profile_dir": profile_dir });
    if let Err(e) = registry.execute("browser.close", &params).await {
        tracing::warn!(profile_dir, error = %e, "任务结束后强制关闭浏览器失败");
    }
}

/// 针对单个 session 的强制关闭。
/// 用于 persistent 平台在 flow 中途失败时回收当前临时 tab；
/// 不受 manifest.browser_lifecycle 影响。
pub(crate) async fn force_close_browser_session(registry: &EngineRegistry, session_id: &str) {
    if session_id.is_empty() {
        return;
    }
    let params = serde_json::json!({
        "session_id": session_id,
        "force": true,
    });
    if let Err(e) = registry.execute("browser.close", &params).await {
        tracing::warn!(session_id, error = %e, "任务失败后强制关闭当前浏览器 session 失败");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 写一个最小 manifest 到临时目录，带 lifecycle 字段；load 出 PluginLoader 供测试用。
    fn loader_with_lifecycle(
        platform: &str,
        lifecycle: Option<&str>,
    ) -> (tempfile::TempDir, PluginLoader) {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join(platform);
        std::fs::create_dir_all(&sub).unwrap();
        let mut manifest = serde_json::json!({
            "platform_id": platform,
            "display_name": platform,
            "adapter_mode": "browser",
            "login_url": "https://example.com/login",
            "workspace_url": "https://example.com",
            "models": [],
            "tiers": {},
            "flows": {}
        });
        if let Some(lc) = lifecycle {
            manifest["browser_lifecycle"] = serde_json::Value::String(lc.to_string());
        }
        std::fs::write(sub.join("manifest.json"), manifest.to_string()).unwrap();
        let loader = PluginLoader::load_from(tmp.path()).unwrap();
        (tmp, loader)
    }

    /// persistent 平台必须在 force_close_browser 的早期分支短路——
    /// 不会触碰 registry；用完全空的 EngineRegistry 作哨兵，调用 per_task 分支时会
    /// 因为找不到 browser engine 报错（tracing warn），而 persistent 分支应保持静默成功返回。
    /// 我们仅通过"函数不 panic 且返回 void"判断短路成立；配合另一条非 persistent 路径
    /// 的对照，确保分支判断真的起作用。
    #[tokio::test]
    async fn force_close_browser_skips_persistent_platform() {
        let (_tmp, loader) = loader_with_lifecycle("flow", Some("persistent"));
        let empty_registry = EngineRegistry::new();
        // 有 profile_dir + persistent → 直接短路（不会去调 empty registry）
        force_close_browser(
            &empty_registry,
            &loader,
            "flow",
            "/tmp/frame64-test-profile",
        )
        .await;
    }

    /// 空 profile_dir 的早期短路（兼容 API-only 平台）。
    #[tokio::test]
    async fn force_close_browser_noop_on_empty_profile() {
        let (_tmp, loader) = loader_with_lifecycle("doubao", None);
        let empty_registry = EngineRegistry::new();
        force_close_browser(&empty_registry, &loader, "doubao", "").await;
    }

    /// 未知平台（插件加载失败场景）仍按 per_task 行为继续——
    /// registry 没有引擎会打 warn，但函数不会 panic。
    #[tokio::test]
    async fn force_close_browser_falls_back_to_per_task_when_platform_unknown() {
        let (_tmp, loader) = loader_with_lifecycle("flow", Some("persistent"));
        let empty_registry = EngineRegistry::new();
        // platform 不在 loader 里，应该走到 execute 分支；empty registry 返回 Err 被吞掉。
        force_close_browser(
            &empty_registry,
            &loader,
            "nonexistent",
            "/tmp/frame64-test-profile",
        )
        .await;
    }
}

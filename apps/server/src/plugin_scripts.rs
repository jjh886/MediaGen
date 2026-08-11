use std::path::{Path, PathBuf};

const SHARED_PREFIX: &str = "@shared/";

/// 读取插件声明的脚本列表。普通名称从当前插件 scripts/ 读取，@shared/ 从插件公共目录读取。
/// 路径解析完全与平台无关，平台选择哪些脚本仍由 manifest 决定。
pub(crate) fn read_plugin_scripts(
    plugin_dir: &Path,
    references: &[String],
) -> Result<String, String> {
    read_from_scripts_dir(&plugin_dir.join("scripts"), references)
}

pub(crate) fn read_from_scripts_dir(
    scripts_dir: &Path,
    references: &[String],
) -> Result<String, String> {
    let mut combined = String::new();
    for reference in references {
        let path = resolve_script_path(scripts_dir, reference)?;
        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("读取插件脚本失败 {}: {error}", path.display()))?;
        combined.push_str(&content);
        combined.push_str("\n;\n");
    }
    Ok(combined)
}

pub(crate) fn is_safe_script_reference(reference: &str) -> bool {
    script_name(reference).is_some()
}

fn resolve_script_path(scripts_dir: &Path, reference: &str) -> Result<PathBuf, String> {
    let name = script_name(reference).ok_or_else(|| format!("插件脚本引用不安全: {reference}"))?;
    if reference.starts_with(SHARED_PREFIX) {
        let plugin_dir = scripts_dir
            .parent()
            .ok_or_else(|| format!("插件脚本目录无效: {}", scripts_dir.display()))?;
        let plugins_dir = plugin_dir
            .parent()
            .ok_or_else(|| format!("插件目录无效: {}", plugin_dir.display()))?;
        return Ok(plugins_dir.join("_shared").join(name));
    }
    Ok(scripts_dir.join(name))
}

fn script_name(reference: &str) -> Option<&str> {
    let value = reference.trim();
    let name = value.strip_prefix(SHARED_PREFIX).unwrap_or(value);
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || !name.ends_with(".js")
    {
        return None;
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_local_and_shared_scripts_in_declared_order() {
        let root =
            std::env::temp_dir().join(format!("mediagen-plugin-scripts-{}", uuid::Uuid::new_v4()));
        let plugin = root.join("demo");
        std::fs::create_dir_all(plugin.join("scripts")).unwrap();
        std::fs::create_dir_all(root.join("_shared")).unwrap();
        std::fs::write(root.join("_shared/helper.js"), "const helper = 1").unwrap();
        std::fs::write(plugin.join("scripts/main.js"), "helper + 1").unwrap();

        let script = read_plugin_scripts(
            &plugin,
            &["@shared/helper.js".to_string(), "main.js".to_string()],
        )
        .unwrap();

        assert!(script.find("const helper").unwrap() < script.find("helper + 1").unwrap());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_parent_directory_traversal() {
        assert!(!is_safe_script_reference("../secret.js"));
        assert!(!is_safe_script_reference("@shared/../secret.js"));
        assert!(is_safe_script_reference("@shared/helper.js"));
    }
}

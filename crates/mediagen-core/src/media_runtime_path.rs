use std::path::{Path, PathBuf};

fn bundled_tool_binary(root: &Path, name: &str) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let binary = root.join("ffmpeg-runtime").join(name);
    binary.is_file().then_some(binary)
}

/// 安装包不依赖 PATH；源码运行仍可显式指定或使用系统 FFmpeg。
pub fn resolve_ffmpeg_binary() -> PathBuf {
    resolve_media_binary("ffmpeg", "MEDIAGEN_FFMPEG")
}

pub fn resolve_ffprobe_binary() -> PathBuf {
    resolve_media_binary("ffprobe", "MEDIAGEN_FFPROBE")
}

fn resolve_media_binary(name: &str, env_key: &str) -> PathBuf {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
            if let Some(root) = parent.parent() {
                roots.push(root.to_path_buf());
            }
        }
    }
    if cfg!(target_os = "linux") {
        roots.push(PathBuf::from("/opt/mediagen"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.clone());
        roots.push(cwd.join("dist-local/MediaGen-local"));
    }
    resolve_from_roots(name, std::env::var_os(env_key), &roots)
}

fn resolve_from_roots(
    name: &str,
    override_path: Option<std::ffi::OsString>,
    roots: &[PathBuf],
) -> PathBuf {
    if let Some(binary) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(binary);
    }
    roots
        .iter()
        .find_map(|root| bundled_tool_binary(root, name))
        .unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn installed_runtime_is_found_without_system_path_and_override_is_respected() {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "mediagen-ffmpeg-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let runtime = root.join("ffmpeg-runtime");
        std::fs::create_dir_all(&runtime).unwrap();
        let roots = [root.join("bin"), root.clone()];
        for name in ["ffmpeg", "ffprobe"] {
            let binary = runtime.join(if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.to_string()
            });
            std::fs::write(&binary, []).unwrap();
            assert_eq!(resolve_from_roots(name, None, &roots), binary);
            assert_eq!(resolve_from_roots(name, Some("".into()), &roots), binary);
            assert_eq!(
                resolve_from_roots(name, Some("custom-tool".into()), &roots),
                PathBuf::from("custom-tool")
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_run_without_runtime_uses_system_ffmpeg() {
        for name in ["ffmpeg", "ffprobe"] {
            assert_eq!(resolve_from_roots(name, None, &[]), PathBuf::from(name));
        }
    }
}

use std::path::{Path, PathBuf};

/// 把一个 CloakBrowser runtime 目录转换成当前系统可能存在的浏览器可执行文件路径。
pub fn cloakbrowser_runtime_candidates(runtime_dir: &Path) -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        return vec![
            runtime_dir.join("Chromium.app/Contents/MacOS/Chromium"),
            runtime_dir.join("CloakBrowser.app/Contents/MacOS/CloakBrowser"),
            runtime_dir.join("Cloak Browser.app/Contents/MacOS/CloakBrowser"),
        ];
    }
    if cfg!(target_os = "windows") {
        return vec![
            runtime_dir.join("chrome.exe"),
            runtime_dir.join("CloakBrowser/chrome.exe"),
            runtime_dir.join("CloakBrowser/Application/cloakbrowser.exe"),
            runtime_dir.join("cloakbrowser.exe"),
        ];
    }
    vec![
        runtime_dir.join("chrome"),
        runtime_dir.join("cloakbrowser/chrome"),
        runtime_dir.join("cloakbrowser"),
    ]
}

pub fn bundled_cloakbrowser_candidates(root: &Path) -> Vec<PathBuf> {
    cloakbrowser_runtime_candidates(&root.join("browser-runtime"))
}

pub fn path_or_runtime_candidates(path: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![path.to_path_buf()];
    candidates.extend(cloakbrowser_runtime_candidates(path));
    candidates
}

pub fn first_existing_browser_binary(
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::{bundled_cloakbrowser_candidates, cloakbrowser_runtime_candidates};
    use std::path::Path;

    #[test]
    fn bundled_candidates_are_under_browser_runtime() {
        let candidates = bundled_cloakbrowser_candidates(Path::new("/opt/mediagen"));
        assert!(candidates
            .iter()
            .all(|path| path.starts_with("/opt/mediagen/browser-runtime")));
    }

    #[test]
    fn runtime_candidates_include_platform_binary() {
        let candidates = cloakbrowser_runtime_candidates(Path::new("/runtime"));
        let text = candidates
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        if cfg!(target_os = "macos") {
            assert!(text.contains("Chromium.app/Contents/MacOS/Chromium"));
        } else if cfg!(target_os = "windows") {
            assert!(text.contains("chrome.exe"));
        } else {
            assert!(text.contains("/runtime/chrome"));
        }
    }
}

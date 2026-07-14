use std::path::Path;

pub(crate) fn resolve_profile_dir(
    profile_root: &Path,
    session_id: &str,
    requested: Option<&str>,
) -> String {
    requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| profile_root.join(session_id).display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_requested_profile_dir_when_present() {
        assert_eq!(
            resolve_profile_dir(Path::new("/profiles"), "session-1", Some(" /tmp/profile ")),
            "/tmp/profile"
        );
    }

    #[test]
    fn falls_back_to_profile_root_and_session_id() {
        assert_eq!(
            resolve_profile_dir(Path::new("/profiles"), "session-1", Some(" ")),
            "/profiles/session-1"
        );
    }
}

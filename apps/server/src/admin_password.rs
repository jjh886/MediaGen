use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand::rngs::OsRng;
pub(crate) fn validate_username(value: &str) -> Result<(), String> {
    if !(3..=64).contains(&value.len())
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    {
        return Err("用户名为 3–64 位字母、数字、下划线、点或短横线".into());
    }
    Ok(())
}
pub(crate) fn validate_password(value: &str) -> Result<(), String> {
    if value.chars().count() < 12 || value.len() > 1024 || value.trim().is_empty() {
        return Err("密码至少 12 个字符，且不能超过 1024 字节".into());
    }
    Ok(())
}
pub(crate) fn hash(value: &str) -> Result<String, String> {
    Argon2::default()
        .hash_password(value.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|hash| hash.to_string())
        .map_err(|_| "密码处理失败".into())
}
pub(crate) fn verify(value: &str, hash: &str) -> bool {
    if let Some(legacy) = hash.strip_prefix("legacy-sha256:") {
        return equal(
            legacy.as_bytes(),
            crate::access_control::hash_secret(value).as_bytes(),
        );
    }
    PasswordHash::new(hash).ok().is_some_and(|parsed| {
        Argon2::default()
            .verify_password(value.as_bytes(), &parsed)
            .is_ok()
    })
}
pub(crate) fn equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

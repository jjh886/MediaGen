use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rand::RngCore;

const KEY_FILE_NAME: &str = "generation-queue.key";

/// 队列会跨重启保存调用方凭据，因此必须先加密，不能把 API Key 明文写进数据库。
pub(crate) struct CredentialCipher {
    cipher: Aes256Gcm,
}

impl CredentialCipher {
    pub(crate) fn new(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
        let key_path = data_dir.join(KEY_FILE_NAME);
        let key = if key_path.exists() {
            std::fs::read(&key_path).map_err(|error| error.to_string())?
        } else {
            let mut key = vec![0u8; 32];
            rand::thread_rng().fill_bytes(&mut key);
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&key_path) {
                Ok(mut file) => {
                    file.write_all(&key).map_err(|_| "安全存储密钥写入失败")?;
                    file.sync_all().map_err(|_| "安全存储密钥落盘失败")?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    key = std::fs::read(&key_path).map_err(|_| "安全存储密钥读取失败")?;
                }
                Err(_) => return Err("安全存储密钥创建失败".into()),
            }
            set_private_permissions(&key_path)?;
            key
        };
        set_private_permissions(&key_path)?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|_| "生成队列凭据密钥长度无效".to_string())?;
        Ok(Self { cipher })
    }

    #[cfg(test)]
    pub(crate) fn ephemeral() -> Self {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        Self {
            cipher: Aes256Gcm::new_from_slice(&key).expect("固定 AES-256 密钥长度"),
        }
    }

    pub(crate) fn encrypt(&self, plaintext: &str) -> Result<String, String> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
            .map_err(|_| "保存生成队列凭据失败".to_string())?;
        let mut payload = nonce_bytes.to_vec();
        payload.extend(ciphertext);
        Ok(STANDARD.encode(payload))
    }

    pub(crate) fn decrypt(&self, encoded: &str) -> Result<String, String> {
        let payload = STANDARD
            .decode(encoded)
            .map_err(|_| "读取生成队列凭据失败".to_string())?;
        if payload.len() <= 12 {
            return Err("生成队列凭据数据无效".to_string());
        }
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(&payload[..12]), &payload[12..])
            .map_err(|_| "解密生成队列凭据失败".to_string())?;
        String::from_utf8(plaintext).map_err(|_| "生成队列凭据编码无效".to_string())
    }
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(0o600);
    std::fs::set_permissions(path, permissions).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_credentials_survive_restart_without_plaintext() {
        let root = std::env::temp_dir().join(format!(
            "mediagen-generation-cipher-{}",
            uuid::Uuid::new_v4()
        ));
        let cipher = CredentialCipher::new(&root).unwrap();
        let encrypted = cipher.encrypt("bearer-secret").unwrap();

        assert!(!encrypted.contains("bearer-secret"));
        assert_eq!(
            CredentialCipher::new(&root)
                .unwrap()
                .decrypt(&encrypted)
                .unwrap(),
            "bearer-secret"
        );
    }
}

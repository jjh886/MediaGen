use async_trait::async_trait;
use base64::prelude::{Engine as _, BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD};
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::engine::AtomExecutor;
use crate::error::AppError;
use crate::types::StepResult;

pub struct CryptoEngine;

impl CryptoEngine {
    pub fn new() -> Self {
        Self
    }

    async fn random_base64url(&self, params: &Value) -> Result<StepResult, AppError> {
        let bytes_len = params["bytes"].as_u64().unwrap_or(32).clamp(1, 256) as usize;
        let mut bytes = vec![0u8; bytes_len];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Ok(StepResult {
            success: true,
            data: json!({ "value": BASE64_URL_SAFE_NO_PAD.encode(bytes) }),
            error: None,
        })
    }

    async fn sha256_base64url(&self, params: &Value) -> Result<StepResult, AppError> {
        let text = params["text"]
            .as_str()
            .ok_or_else(|| AppError::PlatformError("crypto.sha256_base64url 缺少 text".into()))?;
        let digest = Sha256::digest(text.as_bytes());
        Ok(StepResult {
            success: true,
            data: json!({ "value": BASE64_URL_SAFE_NO_PAD.encode(digest) }),
            error: None,
        })
    }

    async fn base64url_decode_utf8(&self, params: &Value) -> Result<StepResult, AppError> {
        let text = params["text"].as_str().ok_or_else(|| {
            AppError::PlatformError("crypto.base64url_decode_utf8 缺少 text".into())
        })?;
        let bytes = BASE64_URL_SAFE_NO_PAD
            .decode(text)
            .or_else(|_| BASE64_STANDARD.decode(text))
            .map_err(|e| AppError::PlatformError(format!("base64url 解码失败: {e}")))?;
        let value = String::from_utf8(bytes)
            .map_err(|e| AppError::PlatformError(format!("base64url 不是 UTF-8: {e}")))?;
        Ok(StepResult {
            success: true,
            data: json!({ "value": value }),
            error: None,
        })
    }
}

#[async_trait]
impl AtomExecutor for CryptoEngine {
    fn handles(&self, action_prefix: &str) -> bool {
        action_prefix == "crypto"
    }

    async fn execute(&self, action: &str, params: &Value) -> Result<StepResult, AppError> {
        match action {
            "crypto.random_base64url" => self.random_base64url(params).await,
            "crypto.sha256_base64url" => self.sha256_base64url(params).await,
            "crypto.base64url_decode_utf8" => self.base64url_decode_utf8(params).await,
            _ => Err(AppError::PlatformError(format!(
                "未知的 crypto 操作: {action}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sha256_base64url_matches_pkce_vector_without_pkce_semantics() {
        let result = CryptoEngine::new()
            .execute(
                "crypto.sha256_base64url",
                &json!({ "text": "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk" }),
            )
            .await
            .unwrap();

        assert_eq!(
            result.data["value"],
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}

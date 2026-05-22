use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("插件未找到: {0}")]
    PluginNotFound(String),

    #[error("模型未找到: {0}")]
    ModelNotFound(String),

    #[error("无可用账号: {0}")]
    AccountUnavailable(String),

    #[error("平台错误: {0}")]
    PlatformError(String),

    #[error("生成超时")]
    GenerationTimeout,

    #[error("浏览器错误: {0}")]
    BrowserError(String),

    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("HTTP 错误: {0}")]
    Http(#[from] reqwest::Error),

    #[error("序列化错误: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_plugin_not_found() {
        let e = AppError::PluginNotFound("flow".to_string());
        assert_eq!(e.to_string(), "插件未找到: flow");
    }

    #[test]
    fn display_model_not_found() {
        let e = AppError::ModelNotFound("v3".to_string());
        assert_eq!(e.to_string(), "模型未找到: v3");
    }

    #[test]
    fn display_account_unavailable() {
        let e = AppError::AccountUnavailable("doubao".to_string());
        assert_eq!(e.to_string(), "无可用账号: doubao");
    }

    #[test]
    fn display_platform_error() {
        let e = AppError::PlatformError("rate limit".to_string());
        assert_eq!(e.to_string(), "平台错误: rate limit");
    }

    #[test]
    fn display_generation_timeout() {
        let e = AppError::GenerationTimeout;
        assert_eq!(e.to_string(), "生成超时");
    }

    #[test]
    fn display_browser_error() {
        let e = AppError::BrowserError("CDP disconnected".to_string());
        assert_eq!(e.to_string(), "浏览器错误: CDP disconnected");
    }

    #[test]
    fn from_rusqlite_error() {
        let sqlite_err = rusqlite::Error::QueryReturnedNoRows;
        let e: AppError = sqlite_err.into();
        assert!(e.to_string().starts_with("数据库错误:"));
    }

    #[test]
    fn from_serde_json_error() {
        let json_err = serde_json::from_str::<String>("not-json").unwrap_err();
        let e: AppError = json_err.into();
        assert!(e.to_string().starts_with("序列化错误:"));
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let e: AppError = io_err.into();
        assert!(e.to_string().starts_with("IO 错误:"));
    }

    #[test]
    fn serialize_to_json_string() {
        let e = AppError::GenerationTimeout;
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, "\"生成超时\"");
    }

    #[test]
    fn serialize_variant_with_message() {
        let e = AppError::PluginNotFound("test".to_string());
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, "\"插件未找到: test\"");
    }
}

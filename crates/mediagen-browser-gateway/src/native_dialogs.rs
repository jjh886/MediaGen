use std::sync::Arc;

use serde_json::{json, Value};

use crate::cdp::CdpClient;

const WEBAUTHN_GUARD_SCRIPT: &str = r#"
(() => {
  if (window.__mediagenWebAuthnGuardInstalled) return;
  window.__mediagenWebAuthnGuardInstalled = true;
  const rejectRequest = () =>
    Promise.reject(new DOMException('Remote browser closed the native credential dialog.', 'NotAllowedError'));
  const patch = (target, key) => {
    try {
      const original = target && target[key];
      if (typeof original !== 'function') return;
      Object.defineProperty(target, key, {
        configurable: true,
        value(...args) {
          if (args[0] && args[0].publicKey) return rejectRequest();
          return original.apply(this, args);
        },
      });
    } catch (_) {}
  };
  patch(window.CredentialsContainer && window.CredentialsContainer.prototype, 'get');
  patch(window.CredentialsContainer && window.CredentialsContainer.prototype, 'create');
  patch(navigator.credentials, 'get');
  patch(navigator.credentials, 'create');
})();
"#;

pub(crate) async fn install_native_dialog_guards(client: &Arc<CdpClient>) {
    // passkey/WebAuthn 弹窗不是 DOM。这里提前拒绝 publicKey 凭据请求，避免系统原生弹窗阻塞会话。
    let _ = client
        .send("WebAuthn.enable", json!({ "enableUI": false }))
        .await;
    let _ = client
        .send(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": WEBAUTHN_GUARD_SCRIPT }),
        )
        .await;
    let _ = client
        .send(
            "Runtime.evaluate",
            json!({ "expression": WEBAUTHN_GUARD_SCRIPT, "returnByValue": true }),
        )
        .await;
}

pub(crate) async fn dismiss_javascript_dialog(client: &Arc<CdpClient>, params: &Value) {
    let dialog_type = params
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let accept = dialog_type == "beforeunload";
    let _ = client
        .send("Page.handleJavaScriptDialog", json!({ "accept": accept }))
        .await;
}

pub(crate) async fn continue_paused_request(client: &Arc<CdpClient>, params: &Value) {
    let Some(request_id) = params.get("requestId").and_then(Value::as_str) else {
        return;
    };
    let _ = client
        .send("Fetch.continueRequest", json!({ "requestId": request_id }))
        .await;
}

#[cfg(test)]
mod tests {
    use super::WEBAUTHN_GUARD_SCRIPT;

    #[test]
    fn webauthn_guard_rejects_public_key_credentials_without_text_matching() {
        assert!(WEBAUTHN_GUARD_SCRIPT.contains("publicKey"));
        assert!(WEBAUTHN_GUARD_SCRIPT.contains("'create'"));
        assert!(WEBAUTHN_GUARD_SCRIPT.contains("NotAllowedError"));
        assert!(!WEBAUTHN_GUARD_SCRIPT.contains("document.body"));
        assert!(!WEBAUTHN_GUARD_SCRIPT.contains("innerText"));
    }
}

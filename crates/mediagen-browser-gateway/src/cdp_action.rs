use crate::{BrowserCdpRequest, BrowserCdpResponse, BrowserGateway, BrowserGatewayError};

impl BrowserGateway {
    pub async fn send_cdp_command(
        &self,
        id: &str,
        request: BrowserCdpRequest,
    ) -> Result<BrowserCdpResponse, BrowserGatewayError> {
        let method = request.method.trim();
        if method.is_empty() {
            return Err(BrowserGatewayError::Cdp("CDP method 不能为空".to_string()));
        }
        let data = self
            .session_client(id)
            .await?
            .send(method, request.params)
            .await?;
        Ok(BrowserCdpResponse { ok: true, data })
    }
}

mod chat;
mod error;
mod tls;
mod translate;

pub use chat::SessionStream;
pub use error::GatewayError;

use std::path::PathBuf;
use std::sync::Arc;

use model::{HarnessId, SessionId, session::SessionSummary};
use rustls::ClientConfig;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub base_url: String,
    pub ca_pem: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Gateway {
    http: reqwest::Client,
    tls: Arc<ClientConfig>,
    base_url: String,
}

#[derive(Serialize)]
struct CreateSession<'a> {
    harness: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<&'a str>,
}

impl Gateway {
    pub fn connect(endpoint: &Endpoint) -> Result<Self, GatewayError> {
        let tls = Arc::new(tls::client_config(&endpoint.ca_pem)?);
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(ClientConfig::clone(&tls))
            .build()
            .map_err(GatewayError::Http)?;
        Ok(Self {
            http,
            tls,
            base_url: endpoint.base_url.trim_end_matches('/').to_owned(),
        })
    }

    pub async fn sessions(&self, harness: &HarnessId) -> Result<Vec<SessionSummary>, GatewayError> {
        let url = format!("{}/api/harnesses/{}/sessions", self.base_url, harness.as_str());
        let list: wire::harness::HarnessSessionList = self.get_json(&url).await?;
        Ok(list.sessions.into_iter().map(translate::summary).collect())
    }

    pub async fn start_session(
        &self,
        harness: &HarnessId,
        model: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<SessionSummary, GatewayError> {
        let url = format!("{}/api/harness-sessions", self.base_url);
        let body = CreateSession {
            harness: harness.as_str(),
            model,
            cwd,
        };
        let created: wire::harness::HarnessSessionSummary =
            Self::read_json(self.http.post(url).json(&body).send().await)?.await?;
        Ok(translate::summary(created))
    }

    pub async fn send_turn(&self, session: &SessionId, text: &str) -> Result<(), GatewayError> {
        let url = format!("{}/api/harness-sessions/{session}/turn", self.base_url);
        let body = serde_json::json!({ "text": text });
        let accepted: wire::gateway::sessions::SessionPostAccepted =
            Self::read_json(self.http.post(url).json(&body).send().await)?.await?;
        if accepted.accepted {
            Ok(())
        } else {
            Err(GatewayError::Refused("turn not accepted".into()))
        }
    }

    pub async fn stop(&self, session: &SessionId) -> Result<(), GatewayError> {
        let url = format!("{}/api/harness-sessions/{session}/stop", self.base_url);
        let reply = self.http.post(url).send().await.map_err(GatewayError::Http)?;
        Self::check(reply).await.map(drop)
    }

    pub async fn open_session(&self, session: &SessionId) -> Result<SessionStream, GatewayError> {
        chat::open(
            &self.base_url,
            Arc::clone(&self.tls),
            &translate::wire_session(session)?,
        )
        .await
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, GatewayError> {
        Self::read_json(self.http.get(url).send().await)?.await
    }

    fn read_json<T: serde::de::DeserializeOwned>(
        sent: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<impl Future<Output = Result<T, GatewayError>>, GatewayError> {
        let reply = sent.map_err(GatewayError::Http)?;
        Ok(async move {
            let reply = Self::check(reply).await?;
            reply.json::<T>().await.map_err(GatewayError::Http)
        })
    }

    async fn check(reply: reqwest::Response) -> Result<reqwest::Response, GatewayError> {
        let status = reply.status();
        if status.is_success() {
            return Ok(reply);
        }
        let detail = reply.text().await.unwrap_or_default();
        Err(GatewayError::Status {
            status: status.as_u16(),
            detail,
        })
    }
}

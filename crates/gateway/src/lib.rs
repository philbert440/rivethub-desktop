mod chat;
mod error;
mod events;
mod terminal;
mod tls;
mod translate;

pub use chat::SessionStream;
pub use error::GatewayError;
pub use events::WorkspaceEvent;
pub use terminal::{TermInput, TermOutput, TerminalStream};

use std::path::PathBuf;
use std::sync::Arc;

use model::{HarnessId, SessionId, session::SessionSummary};
use rustls::ClientConfig;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Identity {
    pub cert_pem: PathBuf,
    pub key_pem: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub base_url: String,
    pub ca_pem: PathBuf,
    pub identity: Option<Identity>,
}

#[derive(Debug, Clone)]
pub struct Gateway {
    http: reqwest::Client,
    tls: Arc<ClientConfig>,
    base_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateSession<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    harness: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_id: Option<&'a str>,
}

impl Gateway {
    pub fn connect(endpoint: &Endpoint) -> Result<Self, GatewayError> {
        let tls = Arc::new(tls::client_config(&endpoint.ca_pem, endpoint.identity.as_ref())?);
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

    pub async fn mesh(&self) -> Result<Vec<model::mesh::Node>, GatewayError> {
        let overview: wire::gateway::devices::MeshOverview =
            self.get_json(&format!("{}/mesh.json", self.base_url)).await?;
        Ok(overview
            .nodes
            .into_iter()
            .map(|node| model::mesh::Node {
                id: node.id,
                name: node.name,
                url: node.url,
                online: node.online,
            })
            .collect())
    }

    pub async fn agents(&self) -> Result<model::agents::Roster, GatewayError> {
        let list: wire::gateway::agents::AgentListResponse =
            self.get_json(&format!("{}/api/agents", self.base_url)).await?;
        Ok(translate::roster(list))
    }

    pub async fn search(&self, query: &str) -> Result<Vec<model::search::Hit>, GatewayError> {
        let url = reqwest::Url::parse_with_params(
            &format!("{}/api/memory/search", self.base_url),
            &[("q", query), ("scope", "messages"), ("limit", "60")],
        )
        .map_err(|err| GatewayError::Refused(format!("search url: {err}")))?;
        let reply = self.http.get(url).send().await.map_err(GatewayError::Http)?;
        let found: wire::gateway::memory::MemorySearchResponse = Self::check(reply)
            .await?
            .json()
            .await
            .map_err(GatewayError::Http)?;
        Ok(translate::hits(found))
    }

    pub async fn start_session(&self, harness: &HarnessId) -> Result<SessionSummary, GatewayError> {
        self.create_session(CreateSession {
            harness: Some(harness.as_str()),
            agent_id: None,
        })
        .await
    }

    pub async fn start_agent(&self, agent_id: &str) -> Result<SessionSummary, GatewayError> {
        self.create_session(CreateSession {
            harness: None,
            agent_id: Some(agent_id),
        })
        .await
    }

    async fn create_session(&self, body: CreateSession<'_>) -> Result<SessionSummary, GatewayError> {
        let url = format!("{}/api/harness-sessions", self.base_url);
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

    pub async fn decide(
        &self,
        session: &SessionId,
        request_id: &str,
        decision: model::session::Decision,
    ) -> Result<(), GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let url = format!(
            "{}/api/harness-sessions/{segment}/approvals/{request_id}",
            self.base_url
        );
        let decision = match decision {
            model::session::Decision::Allow => "allow",
            model::session::Decision::AllowForSession => "allow-session",
            model::session::Decision::Deny => "deny",
        };
        let reply = self
            .http
            .post(url)
            .json(&serde_json::json!({ "decision": decision }))
            .send()
            .await
            .map_err(GatewayError::Http)?;
        Self::check(reply).await.map(drop)
    }

    pub async fn stop(&self, session: &SessionId) -> Result<(), GatewayError> {
        let url = format!("{}/api/harness-sessions/{session}/stop", self.base_url);
        let reply = self.http.post(url).send().await.map_err(GatewayError::Http)?;
        Self::check(reply).await.map(drop)
    }

    pub async fn existing_terminal(&self, session: &SessionId) -> Result<Option<String>, GatewayError> {
        let list: wire::gateway::term::PtyList =
            self.get_json(&format!("{}/term/list", self.base_url)).await?;
        let native = session.native().as_str();
        Ok(list
            .ptys
            .into_iter()
            .find(|pty| pty.session == native && pty.state == wire::gateway::term::PtyState::Running)
            .map(|pty| pty.id))
    }

    pub async fn session_terminal(
        &self,
        session: &SessionId,
        columns: u16,
        rows: u16,
    ) -> Result<String, GatewayError> {
        if let Some(pty) = self.existing_terminal(session).await? {
            return Ok(pty);
        }
        let command = translate::roster_command(session)?;
        let canonical = session.to_string();
        let body = wire::gateway::term::TermSpawnRequest {
            command: Some(command),
            model: None,
            effort: None,
            session: Some(canonical.clone()),
            resume: Some(canonical),
            agent_id: None,
            force: None,
            cols: Some(columns),
            rows: Some(rows),
        };
        let url = format!("{}/term", self.base_url);
        let spawned: wire::gateway::term::TermSpawnResponse =
            Self::read_json(self.http.post(url).json(&body).send().await)?.await?;
        Ok(spawned.id)
    }

    pub async fn watch(&self) -> Result<tokio::sync::mpsc::Receiver<WorkspaceEvent>, GatewayError> {
        events::watch(&self.base_url, Arc::clone(&self.tls)).await
    }

    pub async fn watch_registry(&self) -> Result<tokio::sync::mpsc::Receiver<WorkspaceEvent>, GatewayError> {
        events::watch_registry(&self.base_url, Arc::clone(&self.tls)).await
    }

    pub async fn attach_terminal(&self, pty: &str) -> Result<TerminalStream, GatewayError> {
        terminal::attach(&self.base_url, Arc::clone(&self.tls), pty).await
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

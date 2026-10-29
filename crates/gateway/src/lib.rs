mod chat;
mod error;
mod events;
mod memory;
mod terminal;
mod tls;
mod tool_input;
mod topics;
mod translate;

pub use chat::SessionStream;
pub use error::GatewayError;
pub use events::WorkspaceEvent;
pub use terminal::{TermInput, TermOutput, TerminalStream};
pub use tls::certificates;

use std::path::PathBuf;
use std::sync::Arc;

use model::prompt::{Attachment, Prompt};
use model::{HarnessId, SessionId, session::SessionSummary};
use rustls::ClientConfig;
use serde::{Deserialize, Serialize};

/// The roles a memory search leaves out, so it finds only what people and agents said.
const SAID_ONLY: &str = "tool,system";

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

/// A node's harness list, each entry decoded on its own.
#[derive(Deserialize)]
struct Offerings {
    harnesses: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct Retitle<'a> {
    title: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateSession<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    harness: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
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

    /// A harness this client doesn't know, from an rh newer or older than its own, is left out
    /// rather than failing the whole node; a list it can read none of is an error.
    pub async fn harnesses(&self) -> Result<Vec<model::harness::HarnessOffer>, GatewayError> {
        let url = format!("{}/api/harnesses", self.base_url);
        let list: Offerings = self.get_json(&url).await?;
        let listed = list.harnesses.len();
        let offers: Vec<_> = list
            .harnesses
            .into_iter()
            .filter_map(|raw| serde_json::from_value::<wire::harness::HarnessOffering>(raw).ok())
            .map(translate::offer)
            .collect();
        if offers.is_empty() && listed > 0 {
            return Err(GatewayError::Refused(
                "this app can't read any of the harnesses this node offers".into(),
            ));
        }
        Ok(offers)
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
                sessions: node.sessions,
                version: node.version,
                revision: node.revision,
            })
            .collect())
    }

    pub async fn agents(&self) -> Result<model::agents::Roster, GatewayError> {
        let list: wire::gateway::agents::AgentListResponse =
            self.get_json(&format!("{}/api/agents", self.base_url)).await?;
        Ok(translate::roster(list))
    }

    pub async fn memory_health(&self) -> Result<model::health::MemoryHealth, GatewayError> {
        let (stats, health) = futures::future::try_join(
            self.get_json(&format!("{}/api/memory/stats", self.base_url)),
            self.get_json(&format!("{}/api/memory/health", self.base_url)),
        )
        .await?;
        Ok(translate::memory_health(&stats, &health))
    }

    pub async fn search(&self, query: &str) -> Result<Vec<model::search::Hit>, GatewayError> {
        let url = reqwest::Url::parse_with_params(
            &format!("{}/api/memory/search", self.base_url),
            &[
                ("q", query),
                ("scope", "messages"),
                ("limit", "60"),
                ("exclude_roles", SAID_ONLY),
            ],
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

    pub async fn start_session(
        &self,
        harness: &HarnessId,
        model: Option<&str>,
    ) -> Result<SessionSummary, GatewayError> {
        self.create_session(CreateSession {
            harness: Some(harness.as_str()),
            agent_id: None,
            model,
        })
        .await
    }

    pub async fn start_agent(&self, agent_id: &str) -> Result<SessionSummary, GatewayError> {
        self.create_session(CreateSession {
            harness: None,
            agent_id: Some(agent_id),
            model: None,
        })
        .await
    }

    async fn create_session(&self, body: CreateSession<'_>) -> Result<SessionSummary, GatewayError> {
        let url = format!("{}/api/harness-sessions", self.base_url);
        let created: wire::harness::HarnessSessionSummary =
            Self::read_json(self.http.post(url).json(&body).send().await)?.await?;
        Ok(translate::summary(created))
    }

    pub async fn send_turn(&self, session: &SessionId, prompt: &Prompt) -> Result<(), GatewayError> {
        let url = format!("{}/api/harness-sessions/{session}/turn", self.base_url);
        let body = translate::prompt(prompt)?;
        let accepted: wire::gateway::sessions::SessionPostAccepted =
            Self::read_json(self.http.post(url).json(&body).send().await)?.await?;
        if accepted.accepted {
            Ok(())
        } else {
            Err(GatewayError::Refused("turn not accepted".into()))
        }
    }

    /// Hands `prompt` to the turn running in `session`; false when none is running, so it goes as
    /// a turn instead.
    pub async fn steer(&self, session: &SessionId, prompt: &Prompt) -> Result<bool, GatewayError> {
        let url = format!("{}/api/harness-sessions/{session}/steer", self.base_url);
        let steered: wire::gateway::sessions::SessionSteerResponse =
            Self::read_json(self.http.post(url).json(&translate::steer(prompt)).send().await)?.await?;
        Ok(steered.outcome != wire::harness::SteerOutcome::PromptRequired)
    }

    pub async fn upload(
        &self,
        session: &SessionId,
        name: &str,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> Result<Attachment, GatewayError> {
        let session = translate::wire_session(session)?.to_string();
        let url = reqwest::Url::parse_with_params(
            &format!("{}/api/uploads", self.base_url),
            &[("name", name), ("mediaType", media_type), ("session", &session)],
        )
        .map_err(|err| GatewayError::Refused(format!("upload url: {err}")))?;
        let created: wire::gateway::uploads::UploadCreated =
            Self::read_json(self.http.post(url).body(bytes).send().await)?.await?;
        Ok(translate::attachment(created))
    }

    /// The bytes of an upload rh still holds.
    pub async fn download(&self, upload_id: &str) -> Result<Vec<u8>, GatewayError> {
        let mut url = reqwest::Url::parse(&self.base_url)
            .map_err(|err| GatewayError::Refused(format!("gateway url: {err}")))?;
        url.path_segments_mut()
            .map_err(|()| GatewayError::Refused("gateway url cannot take a path".into()))?
            .extend(["api", "uploads", upload_id]);
        let reply = Self::check(self.http.get(url).send().await.map_err(GatewayError::Http)?).await?;
        Ok(reply.bytes().await.map_err(GatewayError::Http)?.to_vec())
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

    pub async fn tool_output(
        &self,
        session: &SessionId,
        tool_call_id: &str,
        tool_name: &str,
    ) -> Result<Option<model::tools::ToolOutput>, GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let mut url = reqwest::Url::parse(&self.base_url)
            .map_err(|err| GatewayError::Refused(format!("gateway url: {err}")))?;
        url.path_segments_mut()
            .map_err(|()| GatewayError::Refused("gateway url cannot take a path".into()))?
            .extend([
                "api",
                "harness-sessions",
                &segment,
                "tools",
                tool_call_id,
                "output",
            ]);
        let fetched: Result<wire::gateway::sessions::HarnessToolOutput, GatewayError> =
            Self::read_json(self.http.get(url).send().await)?.await;
        match fetched {
            Ok(output) => Ok(Some(tool_input::output(tool_name, &output.text, output.is_error))),
            Err(err) if err.is_missing_route() => Ok(None),
            Err(err) => Err(err),
        }
    }

    pub async fn describe(&self, session: &SessionId) -> Result<model::session::Described, GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let described: wire::gateway::sessions::HarnessSessionResponse = self
            .get_json(&format!("{}/api/harness-sessions/{segment}", self.base_url))
            .await?;
        Ok(model::session::Described {
            summary: translate::summary(described.summary),
            switches: translate::switches(described.switches),
        })
    }

    pub async fn rename(&self, session: &SessionId, title: &str) -> Result<SessionSummary, GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let url = format!("{}/api/harness-sessions/{segment}/title", self.base_url);
        let renamed: wire::harness::HarnessSessionSummary =
            Self::read_json(self.http.put(url).json(&Retitle { title }).send().await)?.await?;
        Ok(translate::summary(renamed))
    }

    pub async fn create_agent(
        &self,
        draft: &model::agents::AgentDraft,
    ) -> Result<model::agents::Agent, GatewayError> {
        use wire::gateway::agents::{AgentPresetCreateRequest, AgentResponse, KnownHarness};
        let body = AgentPresetCreateRequest {
            name: draft.name.clone(),
            color: Some(draft.color_hex()),
            harness_id: draft
                .harness
                .as_ref()
                .and_then(|harness| KnownHarness::try_from(harness.as_str().to_owned()).ok()),
            model: draft.model.clone(),
            effort: draft.effort.clone(),
            system_prompt: draft.instructions.clone(),
            node: draft.node.clone(),
            directory: draft.directory.clone(),
            ..AgentPresetCreateRequest::default()
        };
        let url = format!("{}/api/agents", self.base_url);
        let created: AgentResponse = Self::read_json(self.http.post(url).json(&body).send().await)?.await?;
        Ok(translate::agent(created.agent))
    }

    pub async fn update_agent(
        &self,
        id: &str,
        draft: &model::agents::AgentDraft,
    ) -> Result<model::agents::Agent, GatewayError> {
        use wire::gateway::agents::{AgentPresetUpdate, AgentResponse, KnownHarness};
        let body = AgentPresetUpdate {
            name: Some(draft.name.clone()),
            color: Some(draft.color_hex()),
            harness_id: Some(
                draft
                    .harness
                    .as_ref()
                    .and_then(|harness| KnownHarness::try_from(harness.as_str().to_owned()).ok()),
            ),
            model: Some(draft.model.clone().unwrap_or_default()),
            effort: Some(draft.effort.clone().unwrap_or_default()),
            system_prompt: Some(draft.instructions.clone().unwrap_or_default()),
            directory: draft.directory.clone(),
            ..AgentPresetUpdate::default()
        };
        let url = self.api_url(&["api", "agents", id])?;
        let updated: AgentResponse = Self::read_json(self.http.patch(url).json(&body).send().await)?.await?;
        Ok(translate::agent(updated.agent))
    }

    pub async fn delete_agent(&self, id: &str) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "agents", id])?;
        let reply = self.http.delete(url).send().await.map_err(GatewayError::Http)?;
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
            topic_id: None,
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

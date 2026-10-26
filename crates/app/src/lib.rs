mod feed;
mod session;
mod terminal;

pub use feed::WorkspaceFeed;
pub use session::SessionHandle;
pub use terminal::{TerminalHandle, TerminalState};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gateway::{Endpoint, Gateway, GatewayError, Identity};
use model::HarnessId;
use model::agents::{Agent, Roster};
use model::search::SessionMatch;
use model::session::SessionSummary;
use parking_lot::RwLock;
use tokio::runtime::Handle;

#[derive(Debug)]
struct Inner {
    runtime: Handle,
    local: Gateway,
    ca_pem: PathBuf,
    identity: Option<Identity>,
    local_node: RwLock<Option<String>>,
    remote: RwLock<HashMap<String, Gateway>>,
}

#[derive(Debug, Clone)]
pub struct Hub {
    inner: Arc<Inner>,
}

impl Hub {
    pub fn new(runtime: Handle, local: Gateway, ca_pem: PathBuf, identity: Option<Identity>) -> Self {
        Self {
            inner: Arc::new(Inner {
                runtime,
                local,
                ca_pem,
                identity,
                local_node: RwLock::new(None),
                remote: RwLock::new(HashMap::new()),
            }),
        }
    }

    pub(crate) async fn gateway_for(&self, node: Option<&str>) -> Result<Gateway, GatewayError> {
        let Some(node) = node else {
            return Ok(self.inner.local.clone());
        };
        if self
            .inner
            .local_node
            .read()
            .as_deref()
            .is_some_and(|local| local.eq_ignore_ascii_case(node))
        {
            return Ok(self.inner.local.clone());
        }
        if let Some(gateway) = self.inner.remote.read().get(node) {
            return Ok(gateway.clone());
        }
        let nodes = self.inner.local.mesh().await?;
        let found = nodes.into_iter().find(|candidate| candidate.answers_to(node));
        let Some(found) = found else {
            return Err(GatewayError::UnknownNode(node.to_owned()));
        };
        let gateway = Gateway::connect(&Endpoint {
            base_url: found.url,
            ca_pem: self.inner.ca_pem.clone(),
            identity: self.inner.identity.clone(),
        })?;
        self.inner.remote.write().insert(node.to_owned(), gateway.clone());
        Ok(gateway)
    }

    async fn on_runtime<T, F>(&self, work: F) -> Result<T, GatewayError>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, GatewayError>> + Send + 'static,
    {
        self.inner
            .runtime
            .spawn(work)
            .await
            .unwrap_or_else(|join| Err(GatewayError::Refused(format!("the request stopped: {join}"))))
    }

    pub fn watch_workspace(&self) -> WorkspaceFeed {
        feed::spawn(self)
    }

    pub fn open_session(&self, summary: &SessionSummary) -> SessionHandle {
        session::spawn(
            &self.inner.runtime,
            self.clone(),
            summary.id.clone(),
            summary.node.clone(),
        )
    }

    pub fn open_terminal(&self, summary: &SessionSummary, size: term::GridSize) -> TerminalHandle {
        terminal::spawn(
            &self.inner.runtime,
            self.clone(),
            summary.id.clone(),
            summary.node.clone(),
            size,
        )
    }

    pub async fn has_terminal(&self, summary: &SessionSummary) -> Result<bool, GatewayError> {
        let hub = self.clone();
        let (id, node) = (summary.id.clone(), summary.node.clone());
        self.on_runtime(async move {
            let gateway = hub.gateway_for(node.as_deref()).await?;
            gateway.existing_terminal(&id).await.map(|pty| pty.is_some())
        })
        .await
    }

    pub async fn recent_sessions(&self, harness: HarnessId) -> Result<Vec<SessionSummary>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.sessions(&harness).await })
            .await
    }

    pub async fn agents(&self) -> Result<Roster, GatewayError> {
        let gateway = self.inner.local.clone();
        let roster = self.on_runtime(async move { gateway.agents().await }).await?;
        *self.inner.local_node.write() = Some(roster.node.clone());
        Ok(roster)
    }

    pub async fn search(&self, query: String) -> Result<Vec<SessionMatch>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            gateway
                .search(&query)
                .await
                .map(|hits| model::search::group(hits, &query))
        })
        .await
    }

    pub async fn start_with(&self, agent: Agent) -> Result<SessionSummary, GatewayError> {
        if agent.harness.is_none() {
            return Err(GatewayError::Refused(format!(
                "{} has no harness to start",
                agent.name
            )));
        }
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(agent.node.as_deref()).await?;
            let mut summary = gateway.start_agent(&agent.id).await?;
            summary.node.clone_from(&agent.node);
            Ok(summary)
        })
        .await
    }

    pub async fn start_session(&self, harness: HarnessId) -> Result<SessionSummary, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.start_session(&harness).await })
            .await
    }
}

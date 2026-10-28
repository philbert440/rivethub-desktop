mod feed;
mod memory;
mod notify;
mod prefs;
mod session;
mod status;
mod terminal;
mod topics;

pub use feed::WorkspaceFeed;
pub use memory::{Library, Tidy};
pub use session::SessionHandle;
pub use terminal::{TerminalHandle, TerminalState};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gateway::{Endpoint, Gateway, GatewayError, Identity};
use model::HarnessId;
use model::agents::{Agent, Roster};
use model::harness::HarnessOffer;
use model::prefs::Preferences;
use model::prompt::{Attachment, image_media_type};
use model::search::SessionMatch;
use model::session::SessionSummary;
use parking_lot::{Mutex, RwLock};
use tokio::runtime::Handle;

#[derive(Debug)]
struct Inner {
    runtime: Handle,
    local: Gateway,
    ca_pem: PathBuf,
    identity: Option<Identity>,
    local_node: RwLock<Option<String>>,
    remote: RwLock<HashMap<String, Gateway>>,
    offers: RwLock<HashMap<String, Vec<model::harness::HarnessOffer>>>,
    prefs: RwLock<Preferences>,
    saving: Mutex<()>,
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
                offers: RwLock::new(HashMap::new()),
                prefs: RwLock::new(prefs::load()),
                saving: Mutex::new(()),
            }),
        }
    }

    pub(crate) async fn gateway_for(&self, node: Option<&str>) -> Result<Gateway, GatewayError> {
        let Some(node) = node.filter(|node| !self.is_local(node)) else {
            return Ok(self.inner.local.clone());
        };
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

    fn is_local(&self, node: &str) -> bool {
        self.inner
            .local_node
            .read()
            .as_deref()
            .is_some_and(|local| local.eq_ignore_ascii_case(node))
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

    pub async fn tool_output(
        &self,
        summary: &SessionSummary,
        tool_call_id: String,
        tool_name: String,
    ) -> Result<Option<model::tools::ToolOutput>, GatewayError> {
        let hub = self.clone();
        let session = summary.id.clone();
        let node = summary.node.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(node.as_deref()).await?;
            gateway.tool_output(&session, &tool_call_id, &tool_name).await
        })
        .await
    }

    pub async fn rename(
        &self,
        summary: &SessionSummary,
        title: String,
    ) -> Result<SessionSummary, GatewayError> {
        let hub = self.clone();
        let session = summary.id.clone();
        let node = summary.node.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(node.as_deref()).await?;
            let mut renamed = gateway.rename(&session, &title).await?;
            renamed.node = node;
            Ok(renamed)
        })
        .await
    }

    /// The session as its node describes it now, such as with the title its harness gave it.
    pub async fn summary(&self, summary: &SessionSummary) -> Result<SessionSummary, GatewayError> {
        let hub = self.clone();
        let session = summary.id.clone();
        let node = summary.node.clone();
        self.on_runtime(async move {
            let mut latest = hub.gateway_for(node.as_deref()).await?.summary(&session).await?;
            latest.node = node;
            Ok(latest)
        })
        .await
    }

    /// What the session's harness offers on the node it runs on.
    pub async fn offer(&self, summary: &SessionSummary) -> Result<Option<HarnessOffer>, GatewayError> {
        let hub = self.clone();
        let harness = summary.id.harness().clone();
        let node = summary.node.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(node.as_deref()).await?;
            let offers = hub.offers(node.as_deref(), &gateway).await?;
            Ok(offers.into_iter().find(|offer| offer.id == harness))
        })
        .await
    }

    pub async fn upload(&self, summary: &SessionSummary, path: PathBuf) -> Result<Attachment, GatewayError> {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| GatewayError::Refused(format!("{} is not a file", path.display())))?;
        image_media_type(&name).ok_or_else(|| GatewayError::Refused(format!("{name} isn't an image")))?;
        let read = name.clone();
        let bytes = self
            .on_runtime(async move {
                tokio::task::spawn_blocking(move || std::fs::read(&path))
                    .await
                    .map_err(|join| GatewayError::Refused(format!("reading {read} stopped: {join}")))?
                    .map_err(|err| GatewayError::Refused(format!("reading {read}: {err}")))
            })
            .await?;
        self.upload_bytes(summary, name, bytes).await
    }

    /// Uploads a file that has no path, such as an image pasted from the clipboard.
    pub async fn upload_bytes(
        &self,
        summary: &SessionSummary,
        name: String,
        bytes: Vec<u8>,
    ) -> Result<Attachment, GatewayError> {
        let hub = self.clone();
        let session = summary.id.clone();
        let node = summary.node.clone();
        self.on_runtime(async move {
            let media_type = image_media_type(&name)
                .ok_or_else(|| GatewayError::Refused(format!("{name} isn't an image")))?;
            let gateway = hub.gateway_for(node.as_deref()).await?;
            gateway.upload(&session, &name, media_type, bytes).await
        })
        .await
    }

    /// The bytes of a file sent earlier in the session, from the node that holds it.
    pub async fn download(
        &self,
        summary: &SessionSummary,
        upload_id: String,
    ) -> Result<Vec<u8>, GatewayError> {
        let hub = self.clone();
        let node = summary.node.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(node.as_deref()).await?;
            gateway.download(&upload_id).await
        })
        .await
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

    /// Every session every reachable node lists, across all of its harnesses, newest first.
    pub async fn all_sessions(&self) -> Result<AllSessions, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let local = hub.inner.local_node.read().clone();
            let mut nodes = vec![(None, hub.inner.local.clone())];
            let mesh = hub.inner.local.mesh().await.ok();
            for node in mesh.iter().flatten() {
                let is_local = local.as_deref().is_some_and(|name| node.answers_to(name));
                if node.online
                    && !is_local
                    && let Ok(gateway) = hub.gateway_for(Some(&node.name)).await
                {
                    nodes.push((Some(node.name.clone()), gateway));
                }
            }
            let other_nodes = mesh.map(|_| nodes.iter().filter_map(|(node, _)| node.clone()).collect());
            let listed = futures::future::join_all(nodes.into_iter().map(|(node, gateway)| {
                let hub = hub.clone();
                async move {
                    let sessions = match hub.offers(node.as_deref(), &gateway).await {
                        Ok(offers) => node_sessions(&gateway, &offers).await,
                        Err(err) => Err(err),
                    };
                    (node, sessions)
                }
            }))
            .await;
            let mut all = AllSessions {
                other_nodes,
                ..AllSessions::default()
            };
            let mut seen = HashSet::new();
            for (node, sessions) in listed {
                match sessions {
                    Ok(sessions) => {
                        for mut session in sessions {
                            if seen.insert(session.id.clone()) {
                                if session.node.is_none() {
                                    session.node.clone_from(&node);
                                }
                                all.sessions.push(session);
                            }
                        }
                    }
                    Err(_) => all
                        .unreachable
                        .push(node.unwrap_or_else(|| "this node".to_owned())),
                }
            }
            all.sessions
                .sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
            Ok(all)
        })
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

    /// Starts a harness on `node`, or on this client's own node when it is `None`.
    pub async fn start_session(
        &self,
        node: Option<String>,
        harness: HarnessId,
        model: Option<String>,
    ) -> Result<SessionSummary, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(node.as_deref()).await?;
            let mut summary = gateway.start_session(&harness, model.as_deref()).await?;
            summary.node = node;
            Ok(summary)
        })
        .await
    }

    /// rh only writes an agent on the node that hosts it, so this goes through that node's gateway.
    pub async fn save_agent(
        &self,
        id: Option<String>,
        draft: model::agents::AgentDraft,
    ) -> Result<model::agents::Agent, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(draft.node.as_deref()).await?;
            match id {
                Some(id) => gateway.update_agent(&id, &draft).await,
                None => gateway.create_agent(&draft).await,
            }
        })
        .await
    }

    pub async fn delete_agent(&self, id: String, node: Option<String>) -> Result<(), GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move { hub.gateway_for(node.as_deref()).await?.delete_agent(&id).await })
            .await
    }

    /// The nodes an agent can be hosted on: this node first, then every other online mesh node.
    pub async fn agent_hosts(&self) -> Result<Vec<String>, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let local = hub.inner.local.agents().await?.node;
            *hub.inner.local_node.write() = Some(local.clone());
            let mut names = vec![local.clone()];
            for node in hub.inner.local.mesh().await.unwrap_or_default() {
                if node.online && !node.answers_to(&local) {
                    names.push(node.name);
                }
            }
            Ok(names)
        })
        .await
    }

    /// The harnesses `node` offers, or this client's own node when it is `None`.
    pub async fn harnesses(
        &self,
        node: Option<String>,
    ) -> Result<Vec<model::harness::HarnessOffer>, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(node.as_deref()).await?;
            hub.offers(node.as_deref(), &gateway).await
        })
        .await
    }

    /// A node's harness list, read once per run: asking rh again re-runs its model discovery,
    /// which opens a real session in every ACP harness.
    async fn offers(
        &self,
        node: Option<&str>,
        gateway: &Gateway,
    ) -> Result<Vec<model::harness::HarnessOffer>, GatewayError> {
        let key = node
            .filter(|node| !self.is_local(node))
            .unwrap_or_default()
            .to_owned();
        if let Some(offers) = self.inner.offers.read().get(&key) {
            return Ok(offers.clone());
        }
        let offers = gateway.harnesses().await?;
        self.inner.offers.write().insert(key, offers.clone());
        Ok(offers)
    }
}

#[derive(Debug, Default)]
pub struct AllSessions {
    pub sessions: Vec<SessionSummary>,
    pub unreachable: Vec<String>,
    /// The other online nodes, or `None` when the mesh couldn't be read.
    pub other_nodes: Option<Vec<String>>,
}

async fn node_sessions(
    gateway: &Gateway,
    offers: &[model::harness::HarnessOffer],
) -> Result<Vec<SessionSummary>, GatewayError> {
    let lists = futures::future::join_all(
        offers
            .iter()
            .filter(|offer| offer.lists_sessions)
            .map(|offer| gateway.sessions(&offer.id)),
    )
    .await;
    let mut sessions = Vec::new();
    for list in lists {
        sessions.extend(list?);
    }
    Ok(sessions)
}

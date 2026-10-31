mod backoff;
mod chime;
mod feed;
mod forks;
mod listing;
mod memory;
mod notify;
pub mod omarchy;
mod plugins;
mod prefs;
mod session;
mod status;
mod terminal;
mod topics;
mod trash;
mod visibility;

pub use feed::WorkspaceFeed;
pub use listing::{AllSessions, Missing, SessionListing};
pub use memory::{Library, Tidy};
pub use plugins::{Installed, NodeNotice, PluginWatch};
pub use session::SessionHandle;
pub use terminal::{TerminalHandle, TerminalState};
pub use trash::TrashReview;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gateway::{Endpoint, Gateway, GatewayError, Identity};
use model::agents::{Agent, Roster};
use model::harness::HarnessOffer;
use model::prefs::Preferences;
use model::preview::Previews;
use model::prompt::{Attachment, image_media_type};
use model::search::SessionMatch;
use model::session::{Described, SessionSummary, TaskStop};
use model::{HarnessId, TaskId, TopicId};
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
    forks: Mutex<()>,
    me: RwLock<Option<model::visibility::Me>>,
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
                forks: Mutex::new(()),
                me: RwLock::new(None),
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

    /// The reconnect backoff for a stream that follows `node`, or this client's own node.
    fn backoff(&self, node: Option<&str>) -> backoff::Backoff {
        backoff::Backoff::new(node.is_none_or(|node| self.is_local(node)))
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

    /// Stops the delegated task the session runs, asking the node it runs on.
    pub async fn stop_task(&self, summary: &SessionSummary, task: TaskId) -> Result<TaskStop, GatewayError> {
        let hub = self.clone();
        let node = summary.node.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(node.as_deref()).await?;
            gateway.stop_task(&task).await
        })
        .await
    }

    /// A new chat on the session's node that carries its conversation up to the reply to its
    /// `prompts`th prompt.
    pub async fn fork(&self, summary: &SessionSummary, prompts: u32) -> Result<SessionSummary, GatewayError> {
        let hub = self.clone();
        let session = summary.id.clone();
        let node = summary.node.clone();
        self.on_runtime(async move {
            let mut forked = hub
                .gateway_for(node.as_deref())
                .await?
                .fork(&session, prompts)
                .await?;
            forked.node = node;
            Ok(forked)
        })
        .await
    }

    /// The session's transcript as its node reads it now.
    pub async fn transcript(
        &self,
        summary: &SessionSummary,
    ) -> Result<Vec<model::session::Turn>, GatewayError> {
        let hub = self.clone();
        let session = summary.id.clone();
        let node = summary.node.clone();
        self.on_runtime(async move { hub.gateway_for(node.as_deref()).await?.transcript(&session).await })
            .await
    }

    /// The session as its node describes it now, such as with the title its harness gave it and
    /// whether a turn can switch its model or effort.
    pub async fn describe(&self, summary: &SessionSummary) -> Result<Described, GatewayError> {
        let hub = self.clone();
        let session = summary.id.clone();
        let node = summary.node.clone();
        self.on_runtime(async move {
            let mut latest = hub.gateway_for(node.as_deref()).await?.describe(&session).await?;
            latest.summary.node = node;
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

    /// What each session is doing now, or last said, asked of the node each one lives on. A node
    /// that can't preview (an older rh, or out of reach) just has no previews.
    pub async fn previews(&self, sessions: Vec<SessionSummary>) -> Previews {
        let hub = self.clone();
        self.on_runtime(async move {
            let mut by_node: HashMap<Option<String>, Vec<model::SessionId>> = HashMap::new();
            for session in sessions {
                by_node.entry(session.node).or_default().push(session.id);
            }
            let asked = futures::future::join_all(by_node.into_iter().map(|(node, ids)| {
                let hub = hub.clone();
                async move {
                    let gateway = hub.gateway_for(node.as_deref()).await.ok()?;
                    gateway.previews(&ids).await.ok()
                }
            }))
            .await;
            Ok(asked.into_iter().flatten().flatten().collect())
        })
        .await
        .unwrap_or_default()
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

    /// Starts a harness on `node`, or on this client's own node when it is `None`, in `topic` and
    /// working in `cwd` when given.
    pub async fn start_session(
        &self,
        node: Option<String>,
        harness: HarnessId,
        model: Option<String>,
        topic: Option<TopicId>,
        cwd: Option<String>,
    ) -> Result<SessionSummary, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(node.as_deref()).await?;
            let mut summary = gateway
                .start_session(&harness, model.as_deref(), topic.as_ref(), cwd.as_deref())
                .await?;
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

    /// The label a node's harness offers for `model`, from the offers already read.
    pub fn model_label(&self, node: Option<&str>, harness: &HarnessId, model: &str) -> Option<String> {
        let key = node.filter(|node| !self.is_local(node)).unwrap_or_default();
        self.inner
            .offers
            .read()
            .get(key)?
            .iter()
            .find(|offer| &offer.id == harness)?
            .models
            .iter()
            .find(|choice| choice.id == model)
            .map(|choice| choice.label.clone())
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

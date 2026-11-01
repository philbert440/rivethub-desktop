use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gateway::{GatewayError, WorkspaceEvent};
use model::notice::{Notice, NoticeKind};
use model::session::{SessionStatus, SessionSummary};
use model::{SessionId, TopicId};
use parking_lot::{Mutex, RwLock};
use tokio::sync::{mpsc, watch};
use tokio::task::AbortHandle;

use crate::Hub;
use crate::backoff::Backoff;

/// A stream: the node it comes from (`None` is this client's own node) and which of its feeds.
/// Each keeps its own live state, so one reconnecting doesn't wipe what another said.
type Source = (Option<String>, Feed);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Feed {
    Sessions,
    Registry,
}

/// What one node's streams have said since they last connected.
#[derive(Debug, Default)]
struct Live {
    working: HashSet<String>,
    status: HashMap<SessionId, SessionStatus>,
    blocked: HashMap<SessionId, bool>,
}

#[derive(Debug, Default)]
struct Shared {
    sources: HashMap<Source, Live>,
    notices: Vec<Notice>,
    /// The topics changed since they were last taken.
    topics: HashSet<TopicId>,
}

impl Shared {
    fn find<T>(&self, pick: impl Fn(&Live) -> Option<T>) -> Option<T> {
        self.sources.values().find_map(pick)
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceFeed {
    live: Arc<RwLock<Shared>>,
    changes: watch::Receiver<u64>,
    activity: watch::Receiver<u64>,
    topics: watch::Receiver<u64>,
    remote: Arc<Remote>,
}

impl WorkspaceFeed {
    /// Whether the session is busy, by what its node streamed last, else by its listing.
    pub fn is_active(&self, session: &SessionSummary) -> bool {
        let live = self.live.read();
        let native = session.id.native().as_str();
        live.sources
            .values()
            .any(|source| source.working.contains(native))
            || live
                .find(|source| source.status.get(&session.id).copied())
                .unwrap_or(session.status)
                == SessionStatus::Active
    }

    pub fn needs_you(&self, session: &SessionSummary) -> bool {
        self.live
            .read()
            .find(|source| source.blocked.get(&session.id).copied())
            .unwrap_or(session.blocked)
    }

    /// The notices raised since the last call; each is handed out once.
    pub fn take_notices(&self) -> Vec<Notice> {
        std::mem::take(&mut self.live.write().notices)
    }

    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changes.clone()
    }

    /// Ticks when a session's live state changes or a notice is raised.
    pub fn activity(&self) -> watch::Receiver<u64> {
        self.activity.clone()
    }

    /// Ticks when a node says a topic or its decisions changed.
    pub fn topic_changes(&self) -> watch::Receiver<u64> {
        self.topics.clone()
    }

    /// The topics changed since the last call.
    pub fn take_changed_topics(&self) -> HashSet<TopicId> {
        std::mem::take(&mut self.live.write().topics)
    }

    /// Follows the live feeds of exactly these other nodes: new names get their streams, and
    /// nodes no longer listed have theirs stopped and their live state dropped.
    pub fn follow_nodes(&self, nodes: &[String]) {
        self.remote.follow(nodes);
    }
}

#[derive(Debug)]
struct Signals {
    live: Arc<RwLock<Shared>>,
    changes: watch::Sender<u64>,
    activity: watch::Sender<u64>,
    topics: watch::Sender<u64>,
}

impl Signals {
    fn closed(&self) -> bool {
        self.changes.is_closed() && self.activity.is_closed() && self.topics.is_closed()
    }

    fn reset(&self, source: &Source) {
        self.live.write().sources.remove(source);
        self.activity.send_modify(|count| *count += 1);
        self.changes.send_modify(|count| *count += 1);
    }

    fn forget(&self, source: &Source) {
        if self.live.write().sources.remove(source).is_some() {
            self.activity.send_modify(|count| *count += 1);
        }
    }

    fn apply(&self, source: &Source, event: WorkspaceEvent) {
        let flipped = match event {
            WorkspaceEvent::Changed => {
                self.changes.send_modify(|count| *count += 1);
                return;
            }
            WorkspaceEvent::Activity { native, working } => {
                let mut shared = self.live.write();
                let live = shared.sources.entry(source.clone()).or_default();
                if working {
                    live.working.insert(native)
                } else if live.working.remove(&native) {
                    // A turn ended; harnesses title a session as its turns end.
                    self.changes.send_modify(|count| *count += 1);
                    true
                } else {
                    false
                }
            }
            WorkspaceEvent::Updated { id, status, blocked } => {
                let mut shared = self.live.write();
                let live = shared.sources.entry(source.clone()).or_default();
                let was = live.status.insert(id.clone(), status);
                let was_blocked = live.blocked.get(&id).copied().unwrap_or(false);
                let now_blocked = blocked.unwrap_or(was_blocked);
                if let Some(blocked) = blocked {
                    live.blocked.insert(id.clone(), blocked);
                }
                if was == Some(SessionStatus::Active) && status != SessionStatus::Active {
                    // A turn ended; harnesses title a session as its turns end.
                    self.changes.send_modify(|count| *count += 1);
                }
                if let Some(kind) = NoticeKind::raised_by(was, was_blocked, status, now_blocked) {
                    shared.notices.push(Notice { kind, session: id });
                }
                was != Some(status) || was_blocked != now_blocked
            }
            WorkspaceEvent::TopicChanged(topic) => {
                self.live.write().topics.insert(topic);
                self.topics.send_modify(|count| *count += 1);
                return;
            }
            WorkspaceEvent::Closed => return,
        };
        if flipped {
            self.activity.send_modify(|count| *count += 1);
        }
    }
}

/// The streams of the other online nodes, both of each node's feeds as this node's are followed,
/// stopped when the feed goes away.
#[derive(Debug)]
struct Remote {
    hub: Hub,
    signals: Arc<Signals>,
    tasks: Mutex<HashMap<String, [AbortHandle; 2]>>,
}

impl Remote {
    fn follow(&self, nodes: &[String]) {
        let mut tasks = self.tasks.lock();
        tasks.retain(|node, streams| {
            let keep = nodes.contains(node);
            if !keep {
                for (task, feed) in streams.iter().zip([Feed::Sessions, Feed::Registry]) {
                    task.abort();
                    self.signals.forget(&(Some(node.clone()), feed));
                }
            }
            keep
        });
        for node in nodes {
            if !tasks.contains_key(node) {
                tasks.insert(
                    node.clone(),
                    [Feed::Sessions, Feed::Registry].map(|feed| self.spawn(node, feed)),
                );
            }
        }
    }

    fn spawn(&self, node: &str, feed: Feed) -> AbortHandle {
        let hub = self.hub.clone();
        let name = node.to_owned();
        let task = self.hub.inner.runtime.spawn(follow(
            Arc::clone(&self.signals),
            (Some(name.clone()), feed),
            self.hub.backoff(Some(node)),
            move || {
                let hub = hub.clone();
                let name = name.clone();
                async move {
                    let gateway = hub.gateway_for(Some(&name)).await?;
                    match feed {
                        Feed::Sessions => gateway.watch().await,
                        Feed::Registry => gateway.watch_registry().await,
                    }
                }
            },
        ));
        task.abort_handle()
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        for task in self.tasks.get_mut().values().flatten() {
            task.abort();
        }
    }
}

pub fn spawn(hub: &Hub) -> WorkspaceFeed {
    let live = Arc::new(RwLock::new(Shared::default()));
    let (changes_tx, changes) = watch::channel(0_u64);
    let (activity_tx, activity) = watch::channel(0_u64);
    let (topics_tx, topics) = watch::channel(0_u64);
    let signals = Arc::new(Signals {
        live: Arc::clone(&live),
        changes: changes_tx,
        activity: activity_tx,
        topics: topics_tx,
    });
    let broadcast = hub.inner.local.clone();
    hub.inner.runtime.spawn(follow(
        Arc::clone(&signals),
        (None, Feed::Sessions),
        hub.backoff(None),
        move || {
            let gateway = broadcast.clone();
            async move { gateway.watch().await }
        },
    ));
    let registry = hub.inner.local.clone();
    hub.inner.runtime.spawn(follow(
        Arc::clone(&signals),
        (None, Feed::Registry),
        hub.backoff(None),
        move || {
            let gateway = registry.clone();
            async move { gateway.watch_registry().await }
        },
    ));
    WorkspaceFeed {
        live,
        changes,
        activity,
        topics,
        remote: Arc::new(Remote {
            hub: hub.clone(),
            signals,
            tasks: Mutex::new(HashMap::new()),
        }),
    }
}

async fn follow<C, F>(signals: Arc<Signals>, source: Source, mut retry: Backoff, connect: C)
where
    C: Fn() -> F,
    F: Future<Output = Result<mpsc::Receiver<WorkspaceEvent>, GatewayError>>,
{
    loop {
        if let Ok(mut events) = connect().await {
            retry.reset();
            signals.reset(&source);
            while let Some(event) = events.recv().await {
                if event == WorkspaceEvent::Closed {
                    break;
                }
                signals.apply(&source, event);
            }
            // What the stream said no longer stands, and the listing should find out why it ended.
            signals.reset(&source);
        }
        if signals.closed() {
            return;
        }
        tokio::time::sleep(retry.next_wait()).await;
    }
}

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gateway::{GatewayError, WorkspaceEvent};
use model::SessionId;
use model::session::SessionSummary;
use parking_lot::RwLock;
use tokio::sync::{mpsc, watch};

use crate::Hub;

const FIRST_RETRY: Duration = Duration::from_millis(500);
const LAST_RETRY: Duration = Duration::from_secs(15);

#[derive(Debug, Default)]
struct Live {
    working: HashSet<String>,
    blocked: HashMap<SessionId, bool>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceFeed {
    live: Arc<RwLock<Live>>,
    changes: watch::Receiver<u64>,
    activity: watch::Receiver<u64>,
}

impl WorkspaceFeed {
    pub fn is_working(&self, id: &SessionId) -> bool {
        self.live.read().working.contains(id.native().as_str())
    }

    pub fn needs_you(&self, session: &SessionSummary) -> bool {
        self.live
            .read()
            .blocked
            .get(&session.id)
            .copied()
            .unwrap_or(session.blocked)
    }

    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changes.clone()
    }

    pub fn activity(&self) -> watch::Receiver<u64> {
        self.activity.clone()
    }
}

struct Signals {
    live: Arc<RwLock<Live>>,
    changes: watch::Sender<u64>,
    activity: watch::Sender<u64>,
}

impl Signals {
    fn closed(&self) -> bool {
        self.changes.is_closed() && self.activity.is_closed()
    }

    fn reset(&self) {
        *self.live.write() = Live::default();
        self.activity.send_modify(|count| *count += 1);
        self.changes.send_modify(|count| *count += 1);
    }

    fn apply(&self, event: WorkspaceEvent) {
        let flipped = match event {
            WorkspaceEvent::Changed => {
                self.changes.send_modify(|count| *count += 1);
                return;
            }
            WorkspaceEvent::Activity { native, working } => {
                let mut live = self.live.write();
                if working {
                    live.working.insert(native)
                } else {
                    live.working.remove(&native)
                }
            }
            WorkspaceEvent::Blocked { id, blocked } => {
                self.live.write().blocked.insert(id, blocked) != Some(blocked)
            }
            WorkspaceEvent::Closed => return,
        };
        if flipped {
            self.activity.send_modify(|count| *count += 1);
        }
    }
}

pub fn spawn(hub: &Hub) -> WorkspaceFeed {
    let live = Arc::new(RwLock::new(Live::default()));
    let (changes_tx, changes) = watch::channel(0_u64);
    let (activity_tx, activity) = watch::channel(0_u64);
    let feed = WorkspaceFeed {
        live: Arc::clone(&live),
        changes,
        activity,
    };
    let signals = Arc::new(Signals {
        live,
        changes: changes_tx,
        activity: activity_tx,
    });
    let broadcast = hub.inner.local.clone();
    hub.inner.runtime.spawn(follow(Arc::clone(&signals), move || {
        let gateway = broadcast.clone();
        async move { gateway.watch().await }
    }));
    let registry = hub.inner.local.clone();
    hub.inner.runtime.spawn(follow(signals, move || {
        let gateway = registry.clone();
        async move { gateway.watch_registry().await }
    }));
    feed
}

async fn follow<C, F>(signals: Arc<Signals>, connect: C)
where
    C: Fn() -> F,
    F: Future<Output = Result<mpsc::Receiver<WorkspaceEvent>, GatewayError>>,
{
    let mut retry = FIRST_RETRY;
    loop {
        if let Ok(mut events) = connect().await {
            retry = FIRST_RETRY;
            signals.reset();
            while let Some(event) = events.recv().await {
                if event == WorkspaceEvent::Closed {
                    break;
                }
                signals.apply(event);
            }
        }
        if signals.closed() {
            return;
        }
        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(LAST_RETRY);
    }
}

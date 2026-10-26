use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gateway::WorkspaceEvent;
use model::SessionId;
use parking_lot::RwLock;
use tokio::sync::watch;

use crate::Hub;

const FIRST_RETRY: Duration = Duration::from_millis(500);
const LAST_RETRY: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct WorkspaceFeed {
    working: Arc<RwLock<HashSet<String>>>,
    changes: watch::Receiver<u64>,
    activity: watch::Receiver<u64>,
}

impl WorkspaceFeed {
    pub fn is_working(&self, id: &SessionId) -> bool {
        self.working.read().contains(id.native().as_str())
    }

    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changes.clone()
    }

    pub fn activity(&self) -> watch::Receiver<u64> {
        self.activity.clone()
    }
}

pub fn spawn(hub: &Hub) -> WorkspaceFeed {
    let working = Arc::new(RwLock::new(HashSet::new()));
    let (changes_tx, changes) = watch::channel(0_u64);
    let (activity_tx, activity) = watch::channel(0_u64);
    let feed = WorkspaceFeed {
        working: Arc::clone(&working),
        changes,
        activity,
    };
    let gateway = hub.inner.local.clone();
    hub.inner.runtime.spawn(async move {
        let mut retry = FIRST_RETRY;
        loop {
            if let Ok(mut events) = gateway.watch().await {
                retry = FIRST_RETRY;
                while let Some(event) = events.recv().await {
                    match event {
                        WorkspaceEvent::Snapshot(_) | WorkspaceEvent::Changed => {
                            changes_tx.send_modify(|count| *count += 1);
                        }
                        WorkspaceEvent::Activity {
                            native,
                            working: busy,
                        } => {
                            let mut live = working.write();
                            let flipped = if busy {
                                live.insert(native)
                            } else {
                                live.remove(&native)
                            };
                            drop(live);
                            if flipped {
                                activity_tx.send_modify(|count| *count += 1);
                            }
                        }
                        WorkspaceEvent::Closed => break,
                    }
                }
            }
            if changes_tx.is_closed() && activity_tx.is_closed() {
                return;
            }
            tokio::time::sleep(retry).await;
            retry = (retry * 2).min(LAST_RETRY);
        }
    });
    feed
}

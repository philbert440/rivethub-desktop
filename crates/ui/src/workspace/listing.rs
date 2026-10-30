use std::collections::HashSet;
use std::sync::Arc;

use gateway::GatewayError;
use gpui::Context;
use model::SessionId;
use model::topics::Filing;

use super::{Arrival, Listed, Workspace};

pub(super) type Loaded = (
    Result<app::AllSessions, GatewayError>,
    Result<Filing, GatewayError>,
);

/// Where the workspace's listing stands. One load runs at a time, and a request that arrives during
/// it runs once more after it, so results land in order and requests don't pile up on a slow node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Listing {
    Idle,
    Loading,
    Stale,
}

impl Workspace {
    pub(super) fn load(&mut self, cx: &mut Context<Self>) {
        if self.listing != Listing::Idle {
            self.listing = Listing::Stale;
            return;
        }
        self.listing = Listing::Loading;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded: Loaded = tokio::join!(hub.all_sessions(), hub.topics());
            this.update(cx, |this, cx| {
                let again = this.listing == Listing::Stale;
                this.listing = Listing::Idle;
                this.loaded(loaded, cx);
                if again {
                    this.load(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Reads the topics and what is filed under them again, without listing every node's sessions:
    /// what a topic or filing change alters, shown as soon as rh has it.
    pub(super) fn refile(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let filing = hub.topics().await;
            this.update(cx, |this, cx| {
                match filing {
                    Ok(filing) => {
                        this.take_filing(filing);
                        this.refile_sessions();
                        // A full load already under way read the topics before this change; it
                        // runs again so it can't land over it.
                        if this.listing == Listing::Loading {
                            this.listing = Listing::Stale;
                        }
                    }
                    Err(err) => this.failed = Some(format!("topics: {err}")),
                }
                this.publish(cx);
                this.sync_panel(cx);
                this.sync_step(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn take_filing(&mut self, filing: Filing) {
        self.topics = filing.topics;
        self.placements = filing.placements;
        self.marks = filing.marks;
        self.filed_by = filing.filed_by;
        self.remembered = filing.remembered;
        self.conversations = filing.conversations;
    }

    fn loaded(&mut self, (sessions, topics): Loaded, cx: &mut Context<Self>) {
        let mut problems = Vec::new();
        match sessions {
            Ok(mut all) => {
                if let Some(nodes) = &all.other_nodes {
                    self.feed.follow_nodes(nodes);
                }
                all.keep_from(&self.sessions);
                problems.extend(missing_note(&all.missing, self.hub.local_node().as_deref()));
                self.sessions = all.sessions;
                self.arrival = Arrival::Arrived;
            }
            Err(err) => problems.push(err.to_string()),
        }
        match topics {
            Ok(filing) => self.take_filing(filing),
            Err(err) if err.is_missing_route() => {
                problems.push("Topics need a newer rh on this node.".to_owned());
            }
            // The sessions' note already says this node is out of reach.
            Err(err) if err.is_unreachable() => {}
            Err(err) => problems.push(format!("topics: {err}")),
        }
        self.problem = (!problems.is_empty()).then(|| problems.join(" · "));
        self.refile_sessions();
        self.publish(cx);
        if let Some(pane) = &self.pane {
            let open = pane.read(cx).session_id().clone();
            if let Some(latest) = self.sessions.iter().find(|session| session.id == open).cloned() {
                pane.update(cx, |pane, cx| pane.take_listing(&latest, cx));
            }
        }
        self.subagents.clear();
        for parent in self.sessions.iter().filter_map(|session| session.parent.clone()) {
            *self.subagents.entry(parent).or_default() += 1;
        }
        self.sync_panel(cx);
        self.sync_step(cx);
        self.load_suggestions(cx);
        cx.notify();
    }

    /// The listing's problems and the last failed request, for the page to show.
    pub(super) fn problems(&self) -> Option<String> {
        let parts: Vec<&str> = self
            .failed
            .iter()
            .chain(&self.problem)
            .map(String::as_str)
            .collect();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    fn load_suggestions(&self, cx: &mut Context<Self>) {
        let uncategorized: Vec<SessionId> = self
            .board()
            .uncategorized
            .iter()
            .map(|session| session.id.clone())
            .collect();
        if uncategorized.is_empty() {
            return;
        }
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let found = hub.suggestions(uncategorized).await;
            this.update(cx, |this, cx| {
                if let Ok(found) = found {
                    this.suggestions = found;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn accept_suggestion(&mut self, session: SessionId, cx: &mut Context<Self>) {
        let Some(suggestion) = self.suggestions.remove(&session) else {
            return;
        };
        match suggestion.topic.filter(|topic| self.topic(topic).is_some()) {
            Some(topic) => self.place(session, topic, cx),
            None => self.place_in_new(session, suggestion.name, cx),
        }
    }

    /// Tells whoever follows the workspace, Memory among them, what it lists now.
    pub(super) fn publish(&self, cx: &mut Context<Self>) {
        cx.emit(Arc::new(Listed {
            sessions: self.sessions.clone(),
            topics: self.topics.clone(),
            placements: self.placements.clone(),
            filed_by: self.filed_by.clone(),
            conversations: self.conversations.clone(),
        }));
    }

    /// Rebuilds the sessions the board and topic screen show: the listed ones, then the placed ones
    /// rh remembers that no node lists any more.
    pub(super) fn refile_sessions(&mut self) {
        let listed: HashSet<&SessionId> = self.sessions.iter().map(|session| &session.id).collect();
        let remembered = self
            .remembered
            .iter()
            .filter(|session| !listed.contains(&session.id));
        self.filed_sessions = self.sessions.iter().chain(remembered).cloned().collect();
    }

    /// Something was filed or unfiled elsewhere, such as from Memory: list topics again.
    pub fn refiled(&mut self, cx: &mut Context<Self>) {
        self.refile(cx);
    }
}

/// What the listing couldn't show, by why: the nodes that didn't answer, the ones that won't take
/// this device, and any other failure with what it said.
fn missing_note(missing: &[app::Missing], local: Option<&str>) -> Option<String> {
    let name = |node: &Option<String>| {
        node.as_deref()
            .or(local)
            .map_or_else(|| "this node".to_owned(), str::to_owned)
    };
    let (mut away, mut refused, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    for gap in missing {
        let node = name(&gap.node);
        match &gap.harness {
            _ if gap.error.is_unreachable() => {
                if !away.contains(&node) {
                    away.push(node);
                }
            }
            _ if gap.error.is_refused_device() => {
                if !refused.contains(&node) {
                    refused.push(node);
                }
            }
            None => failed.push(format!("{node}: {}", gap.error)),
            Some(harness) => failed.push(format!("{} on {node}: {}", harness.display_name(), gap.error)),
        }
    }
    let mut parts = Vec::new();
    if !away.is_empty() {
        parts.push(format!("couldn't reach {}", away.join(", ")));
    }
    if !refused.is_empty() {
        parts.push(format!("{} didn't accept this device", refused.join(", ")));
    }
    parts.extend(failed);
    (!parts.is_empty()).then(|| parts.join(" · "))
}

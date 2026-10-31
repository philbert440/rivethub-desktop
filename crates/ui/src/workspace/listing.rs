use std::collections::HashSet;
use std::sync::Arc;

use gateway::GatewayError;
use gpui::{Context, Window};
use model::topics::Filing;
use model::{Altitude, SessionId};

use super::{Arrival, Listed, Workspace};

/// Where the workspace's listing stands. One load runs at a time, and a request that arrives during
/// it runs once more after it, so results land in order and requests don't pile up on a slow node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Listing {
    Idle,
    Loading,
    Stale,
}

impl Workspace {
    /// Lists every node's sessions and the topics again. The topics come first, then each node's
    /// sessions as that node answers, so one that is slow or silent holds back only its own.
    pub(super) fn load(&mut self, cx: &mut Context<Self>) {
        if self.listing != Listing::Idle {
            self.listing = Listing::Stale;
            return;
        }
        self.listing = Listing::Loading;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let (listing, filing, _) = tokio::join!(hub.all_sessions(), hub.topics(), hub.load_me());
            let listing = this
                .update(cx, |this, cx| {
                    this.filed(filing, cx);
                    listing.map_err(|err| this.unlisted = Some(err.to_string())).ok()
                })
                .ok()
                .flatten();
            if let Some(mut listing) = listing {
                while listing.next().await {
                    if this.update(cx, |this, cx| this.listed(&listing.all, cx)).is_err() {
                        return;
                    }
                }
            }
            this.update(cx, |this, cx| {
                let again = this.listing == Listing::Stale;
                this.listing = Listing::Idle;
                this.listing_done(cx);
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
        self.visibility = filing.visibility;
        self.remembered = filing.remembered;
        self.conversations = filing.conversations;
    }

    /// The topics a load read, or why they couldn't be read.
    fn filed(&mut self, topics: Result<Filing, GatewayError>, cx: &mut Context<Self>) {
        self.topics_problem = match topics {
            Ok(filing) => {
                self.take_filing(filing);
                None
            }
            Err(err) if err.is_missing_route() => Some("Topics need a newer rh on this node.".to_owned()),
            // The sessions' note already says this node is out of reach.
            Err(err) if err.is_unreachable() => None,
            Err(err) => Some(format!("topics: {err}")),
        };
        self.show_listing(cx);
    }

    /// The sessions listed so far, taken in as each node answers.
    fn listed(&mut self, all: &app::AllSessions, cx: &mut Context<Self>) {
        if let Some(nodes) = &all.other_nodes {
            self.feed.follow_nodes(nodes);
        }
        self.unlisted = missing_note(&all.missing, self.hub.local_node().as_deref());
        self.sessions = all.shown(&self.sessions);
        if let Some(pane) = &self.pane {
            let open = pane.read(cx).session_id().clone();
            if let Some(latest) = self.sessions.iter().find(|session| session.id == open).cloned() {
                pane.update(cx, |pane, cx| pane.take_listing(&latest, cx));
            }
        }
        self.subagents.clear();
        let subagents = self.sessions.iter().filter(|session| session.inside_parent());
        for parent in subagents.filter_map(|session| session.parent.clone()) {
            *self.subagents.entry(parent).or_default() += 1;
        }
        self.show_listing(cx);
    }

    fn show_listing(&mut self, cx: &mut Context<Self>) {
        self.refile_sessions();
        self.publish(cx);
        self.sync_panel(cx);
        self.show_step(cx);
        cx.notify();
    }

    /// Every node has answered: what the open topic's sessions are doing and where the
    /// uncategorized ones might go are asked once per load.
    fn listing_done(&mut self, cx: &mut Context<Self>) {
        self.arrival = Arrival::Arrived;
        self.sync_step(cx);
        self.load_suggestions(cx);
        cx.notify();
    }

    /// The listing's problems and the last failed request, for the page to show.
    pub(super) fn problems(&self) -> Option<String> {
        let parts: Vec<&str> = self
            .failed
            .iter()
            .chain(&self.unlisted)
            .chain(&self.topics_problem)
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

    /// Deleted sessions leave every list at once, and the page showing one; the listing that
    /// follows has rh's word on them, which lists a session again once it is used.
    pub fn sessions_deleted(&mut self, deleted: &[SessionId], window: &mut Window, cx: &mut Context<Self>) {
        self.sessions.retain(|session| !deleted.contains(&session.id));
        self.remembered.retain(|session| !deleted.contains(&session.id));
        for session in deleted {
            self.placements.remove(session);
            self.suggestions.remove(session);
        }
        self.show_listing(cx);
        match &self.altitude {
            Altitude::Session { session, .. } if deleted.contains(session) => {
                let out = self.altitude.zoom_out();
                self.leave_to(out, window, cx);
            }
            _ => self.load(cx),
        }
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

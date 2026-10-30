use std::collections::HashSet;

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use gateway::{Gateway, GatewayError};
use model::session::SessionSummary;
use model::{HarnessId, SessionId};
use tokio::sync::mpsc;

use crate::Hub;

/// What one node's harnesses list, and the harnesses whose listing failed.
type NodeSessions = (Vec<SessionSummary>, Vec<(HarnessId, GatewayError)>);

/// One node's answer: what it lists, or why it listed nothing. `None` is this client's own node.
#[derive(Debug)]
struct Answer {
    node: Option<String>,
    listed: Result<NodeSessions, GatewayError>,
}

/// A listing under way: what the nodes that have answered list so far, and the answers to come.
#[derive(Debug)]
pub struct SessionListing {
    pub all: AllSessions,
    answers: mpsc::UnboundedReceiver<Answer>,
}

impl SessionListing {
    /// Takes the next node's answer into the listing, as soon as that node gives it; false once
    /// every node asked has answered.
    pub async fn next(&mut self) -> bool {
        let Some(answer) = self.answers.recv().await else {
            return false;
        };
        self.all.take(answer);
        true
    }
}

#[derive(Debug, Default)]
pub struct AllSessions {
    sessions: Vec<SessionSummary>,
    /// The listings that didn't come back.
    pub missing: Vec<Missing>,
    /// The other online nodes, or `None` when the mesh couldn't be read.
    pub other_nodes: Option<Vec<String>>,
    /// The nodes asked that haven't answered yet; `None` is this client's own node.
    pending: Vec<Option<String>>,
    /// Every node asked, in the mesh's order: the missing listings stay in it, whatever order the
    /// nodes answer in.
    asked: Vec<Option<String>>,
    seen: HashSet<SessionId>,
}

/// A listing that didn't come back: a whole node, or one harness on a node that answered.
#[derive(Debug)]
pub struct Missing {
    /// `None` is this client's own node.
    pub node: Option<String>,
    /// `None` when the node itself didn't answer.
    pub harness: Option<HarnessId>,
    pub error: GatewayError,
}

impl Missing {
    fn covers(&self, session: &SessionSummary) -> bool {
        session.node == self.node
            && self
                .harness
                .as_ref()
                .is_none_or(|harness| session.id.harness() == harness)
    }
}

impl AllSessions {
    fn take(&mut self, answer: Answer) {
        self.add(answer);
        let asked = &self.asked;
        self.missing
            .sort_by_key(|missing| asked.iter().position(|node| *node == missing.node));
    }

    fn add(&mut self, Answer { node, listed }: Answer) {
        self.pending.retain(|pending| *pending != node);
        let (sessions, failed) = match listed {
            Ok(listed) => listed,
            Err(error) => {
                self.missing.push(Missing {
                    node,
                    harness: None,
                    error,
                });
                return;
            }
        };
        for (harness, error) in failed {
            self.missing.push(Missing {
                node: node.clone(),
                harness: Some(harness),
                error,
            });
        }
        for mut session in sessions {
            if self.seen.insert(session.id.clone()) {
                if session.node.is_none() {
                    session.node.clone_from(&node);
                }
                self.sessions.push(session);
            }
        }
        self.sessions
            .sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
    }

    /// The sessions listed so far, newest first, with those an earlier listing had where this one
    /// has nothing yet or came back empty-handed: a node still to answer, a node that is
    /// restarting, or a harness that failed to list just now doesn't empty the workspace.
    pub fn shown(&self, earlier: &[SessionSummary]) -> Vec<SessionSummary> {
        let kept = earlier.iter().filter(|session| {
            !self.seen.contains(&session.id)
                && (self.pending.contains(&session.node)
                    || self.missing.iter().any(|missing| missing.covers(session)))
        });
        let mut shown: Vec<SessionSummary> = self.sessions.iter().chain(kept).cloned().collect();
        shown.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
        shown
    }
}

impl Hub {
    /// The nodes to ask, this client's own first, then every other online node the mesh names, and
    /// whether the mesh could be read. Runs on the hub's runtime.
    pub(crate) async fn reachable(&self) -> (Vec<(Option<String>, Gateway)>, bool) {
        // Until this node has named itself, its own mesh entry would be asked again as another
        // node's, over the network.
        let unnamed = self.inner.local_node.read().is_none();
        if unnamed && let Ok(roster) = self.inner.local.agents().await {
            *self.inner.local_node.write() = Some(roster.node);
        }
        let local = self.inner.local_node.read().clone();
        let mut nodes = vec![(None, self.inner.local.clone())];
        let mesh = self.inner.local.mesh().await.ok();
        for node in mesh.iter().flatten() {
            let is_local = local.as_deref().is_some_and(|name| node.answers_to(name));
            if node.online
                && !is_local
                && let Ok(gateway) = self.gateway_for(Some(&node.name)).await
            {
                nodes.push((Some(node.name.clone()), gateway));
            }
        }
        // Without this node's mesh, such as while it restarts, the nodes already reached are
        // still asked directly.
        if mesh.is_none() {
            let known = self.inner.remote.read().clone();
            nodes.extend(
                known
                    .into_iter()
                    .filter(|(name, _)| !self.is_local(name))
                    .map(|(name, gateway)| (Some(name), gateway)),
            );
        }
        (nodes, mesh.is_some())
    }

    /// Every session every reachable node lists, across all of its harnesses. Each node's sessions
    /// join the listing as that node answers, so a slow or silent node holds back only its own.
    pub async fn all_sessions(&self) -> Result<SessionListing, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let (nodes, mesh_read) = hub.reachable().await;
            let all = AllSessions {
                other_nodes: mesh_read.then(|| nodes.iter().filter_map(|(node, _)| node.clone()).collect()),
                pending: nodes.iter().map(|(node, _)| node.clone()).collect(),
                asked: nodes.iter().map(|(node, _)| node.clone()).collect(),
                ..AllSessions::default()
            };
            let (tell, answers) = mpsc::unbounded_channel();
            let mut asked: FuturesUnordered<_> = nodes
                .into_iter()
                .map(|(node, gateway)| {
                    let hub = hub.clone();
                    async move {
                        let listed = match hub.offers(node.as_deref(), &gateway).await {
                            Ok(offers) => Ok(node_sessions(&gateway, &offers).await),
                            Err(err) => Err(err),
                        };
                        Answer { node, listed }
                    }
                })
                .collect();
            hub.inner.runtime.spawn(async move {
                while let Some(answer) = asked.next().await {
                    if tell.send(answer).is_err() {
                        return;
                    }
                }
            });
            Ok(SessionListing { all, answers })
        })
        .await
    }
}

/// Every session the node's harnesses list, and the harnesses whose listing failed.
async fn node_sessions(gateway: &Gateway, offers: &[model::harness::HarnessOffer]) -> NodeSessions {
    let listing: Vec<&HarnessId> = offers
        .iter()
        .filter(|offer| offer.lists_sessions)
        .map(|offer| &offer.id)
        .collect();
    let lists = futures::future::join_all(listing.iter().map(|harness| gateway.sessions(harness))).await;
    let mut sessions = Vec::new();
    let mut failed = Vec::new();
    for (harness, list) in listing.into_iter().zip(lists) {
        match list {
            Ok(list) => sessions.extend(list),
            Err(err) => failed.push((harness.clone(), err)),
        }
    }
    (sessions, failed)
}

#[cfg(test)]
mod tests {
    use model::{HarnessId, NativeSessionId, SessionId};

    use super::*;

    fn session(native: &str, node: Option<&str>, at: i64) -> SessionSummary {
        let id = SessionId::new(HarnessId::new("claude-code"), NativeSessionId::new(native));
        SessionSummary::remembered(id, None, at, node.map(str::to_owned))
    }

    fn shown(all: &AllSessions, earlier: &[SessionSummary]) -> Vec<String> {
        all.shown(earlier)
            .iter()
            .map(|session| session.id.native().as_str().to_owned())
            .collect()
    }

    #[test]
    fn a_node_keeps_its_sessions_until_it_answers_with_its_own() {
        let earlier = [
            session("here", None, 1),
            session("far", Some("far"), 2),
            session("unasked", Some("gone"), 3),
        ];
        let mut all = AllSessions {
            pending: vec![None, Some("far".into())],
            ..AllSessions::default()
        };
        assert_eq!(shown(&all, &earlier), ["far", "here"]);
        all.take(Answer {
            node: None,
            listed: Ok((vec![session("new", None, 4)], Vec::new())),
        });
        assert_eq!(shown(&all, &earlier), ["new", "far"]);
        all.take(Answer {
            node: Some("far".into()),
            listed: Err(GatewayError::TimedOut("far".into())),
        });
        assert_eq!(shown(&all, &earlier), ["new", "far"]);
    }
}

use std::collections::HashMap;

use gateway::GatewayError;
use model::SessionId;
use model::session::SessionSummary;
use model::trash::{Deletion, Trash};

use crate::Hub;

/// What every reachable node counts as obvious trash, newest first, and the nodes that couldn't
/// say: out of reach, or running an rh without the review.
#[derive(Debug, Default)]
pub struct TrashReview {
    pub trash: Vec<Trash>,
    /// `None` is this client's own node.
    pub unasked: Vec<Option<String>>,
}

impl Hub {
    pub async fn trash(&self) -> Result<TrashReview, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let (nodes, _) = hub.reachable().await;
            let answers = futures::future::join_all(nodes.iter().map(|(_, gateway)| gateway.trash())).await;
            let mut review = TrashReview::default();
            for ((node, _), answer) in nodes.into_iter().zip(answers) {
                match answer {
                    Ok(Some(found)) => review.trash.extend(found.into_iter().map(|mut trash| {
                        trash.node.clone_from(&node);
                        trash
                    })),
                    Ok(None) => review.unasked.push(node),
                    Err(err) => {
                        tracing::warn!(%err, node = node.as_deref().unwrap_or("this node"), "the trash review couldn't be read");
                        review.unasked.push(node);
                    }
                }
            }
            review.trash.sort_by_key(|trash| std::cmp::Reverse(trash.updated_at_ms));
            Ok(review)
        })
        .await
    }

    /// Deletes the session on its node: what memory holds of it goes, and the lists leave it out
    /// until it is used again.
    pub async fn delete_session(&self, summary: &SessionSummary) -> Result<Deletion, GatewayError> {
        let hub = self.clone();
        let session = summary.id.clone();
        let node = summary.node.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(node.as_deref()).await?;
            let done = gateway.delete_sessions(std::slice::from_ref(&session)).await?;
            Ok(done
                .into_iter()
                .find_map(|(deleted, outcome)| (deleted == session).then_some(outcome))
                .unwrap_or(Deletion::Failed))
        })
        .await
    }

    /// Deletes each session on the node that counted it, saying what became of each. A node that
    /// can't be reached fails only its own.
    pub async fn delete_trash(&self, trash: Vec<Trash>) -> Vec<(SessionId, Deletion)> {
        let hub = self.clone();
        let all: Vec<SessionId> = trash.iter().map(|doomed| doomed.session.clone()).collect();
        let mut by_node: HashMap<Option<String>, Vec<SessionId>> = HashMap::new();
        for doomed in trash {
            by_node.entry(doomed.node).or_default().push(doomed.session);
        }
        let done = self
            .on_runtime(async move {
                let asked = by_node.into_iter().map(|(node, sessions)| {
                    let hub = hub.clone();
                    async move {
                        let deleted = match hub.gateway_for(node.as_deref()).await {
                            Ok(gateway) => gateway.delete_sessions(&sessions).await,
                            Err(err) => Err(err),
                        };
                        deleted.unwrap_or_else(|err| {
                            tracing::warn!(%err, node = node.as_deref().unwrap_or("this node"), "deleting sessions failed");
                            sessions.into_iter().map(|session| (session, Deletion::Failed)).collect()
                        })
                    }
                });
                Ok(futures::future::join_all(asked).await.into_iter().flatten().collect())
            })
            .await;
        done.unwrap_or_else(|_| {
            all.into_iter()
                .map(|session| (session, Deletion::Failed))
                .collect()
        })
    }
}

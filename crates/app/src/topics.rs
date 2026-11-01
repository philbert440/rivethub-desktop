use gateway::GatewayError;
use std::collections::HashMap;

use model::decisions::{Decision, Group, on_page};
use model::entities::Catalog;
use model::preview::StartupPreview;
use model::topics::{Directory, Filing, Suggestion, Topic, TopicFiling};
use model::{SessionId, TopicId};

use crate::Hub;

impl Hub {
    /// The decisions a root topic's page holds: the root's own, then each sub-topic's.
    pub async fn page_decisions(
        &self,
        topics: Vec<Topic>,
        root: TopicId,
    ) -> Result<Vec<Group>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            let page = on_page(&topics, &root);
            let listed =
                futures::future::try_join_all(page.iter().map(|topic| gateway.decisions(&topic.id))).await?;
            Ok(page
                .into_iter()
                .zip(listed)
                .map(|(topic, decisions)| Group {
                    topic: topic.id.clone(),
                    name: topic.name.clone(),
                    decisions,
                })
                .collect())
        })
        .await
    }

    /// The topic's own decisions, oldest first.
    pub async fn decisions(&self, topic: TopicId) -> Result<Vec<Decision>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.decisions(&topic).await })
            .await
    }

    pub async fn add_decision(&self, topic: TopicId, text: String) -> Result<Decision, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.add_decision(&topic, &text).await })
            .await
    }

    pub async fn edit_decision(
        &self,
        topic: TopicId,
        decision: String,
        text: String,
    ) -> Result<Decision, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.edit_decision(&topic, &decision, &text).await })
            .await
    }

    pub async fn supersede_decision(
        &self,
        topic: TopicId,
        decision: String,
        text: Option<String>,
    ) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            gateway
                .supersede_decision(&topic, &decision, text.as_deref())
                .await
        })
        .await
    }

    /// The topics and what is filed under them: two requests to a node with rh's bulk route, else
    /// one per topic.
    pub async fn topics(&self) -> Result<Filing, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            let (topics, bulk) = futures::future::try_join(gateway.topics(), gateway.filing()).await?;
            let (filed, marks) = if let Some(bulk) = bulk {
                bulk
            } else {
                let each = futures::future::try_join_all(
                    topics.iter().map(|topic| gateway.topic_sessions(&topic.id)),
                )
                .await?;
                let filed = each.into_iter().fold(TopicFiling::default(), |mut all, one| {
                    all.sessions.extend(one.sessions);
                    all.conversations.extend(one.conversations);
                    all
                });
                (filed, gateway.history_marks().await?)
            };
            let mut filing = Filing {
                marks,
                conversations: filed.conversations,
                ..Filing::default()
            };
            for placed in filed.sessions {
                filing
                    .placements
                    .insert(placed.session.clone(), placed.topic.clone());
                filing.filed_by.insert(placed.session.clone(), placed.by);
                filing.remembered.push(placed.summary());
                if let Some(visibility) = placed.visibility {
                    filing.visibility.insert(placed.session, visibility);
                }
            }
            filing.topics = topics;
            Ok(filing)
        })
        .await
    }

    /// Where each of `sessions` might go: one request to a node with rh's bulk route, else one per
    /// session.
    pub async fn suggestions(
        &self,
        sessions: Vec<SessionId>,
    ) -> Result<HashMap<SessionId, Suggestion>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            if let Some(mut all) = gateway.suggestions().await? {
                return Ok(sessions
                    .into_iter()
                    .filter_map(|session| Some((session.clone(), all.remove(&session)?)))
                    .collect());
            }
            let found =
                futures::future::try_join_all(sessions.iter().map(|session| gateway.suggestion(session)))
                    .await?;
            Ok(sessions
                .into_iter()
                .zip(found)
                .filter_map(|(session, suggestion)| Some((session, suggestion?)))
                .collect())
        })
        .await
    }

    /// The entity types and the entities under them.
    pub async fn catalog(&self) -> Result<Catalog, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.catalog().await }).await
    }

    pub async fn create_type(&self, name: String, parent: Option<TopicId>) -> Result<Topic, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.create_type(&name, parent.as_ref()).await })
            .await
    }

    pub async fn set_aliases(&self, topic: TopicId, aliases: Vec<String>) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.set_aliases(&topic, aliases).await })
            .await
    }

    /// Takes a conversation back out of its topic; one the categorizer filed is kept as a correction.
    pub async fn unfile_conversation(&self, conversation: String) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.unfile_conversation(&conversation).await })
            .await
    }

    pub async fn search_topics(&self, query: String) -> Result<Vec<Topic>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.search_topics(&query).await })
            .await
    }

    pub async fn place(&self, session: SessionId, topic: TopicId) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.place(&session, &topic).await })
            .await
    }

    pub async fn unplace(&self, session: SessionId) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.unplace(&session).await })
            .await
    }

    pub async fn place_in_new(&self, session: SessionId, name: String) -> Result<Topic, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            let topic = gateway.create_topic(&name, None).await?;
            gateway.place(&session, &topic.id).await?;
            Ok(topic)
        })
        .await
    }

    /// What a session started in the topic right now would be told.
    pub async fn topic_startup(&self, topic: TopicId) -> Result<StartupPreview, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.topic_startup(&topic).await })
            .await
    }

    /// Creates a topic, under `parent` when given.
    pub async fn create_topic(&self, name: String, parent: Option<TopicId>) -> Result<Topic, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.create_topic(&name, parent.as_ref()).await })
            .await
    }

    /// Closes the topic, or reopens it when `closed` is false.
    pub async fn set_topic_closed(&self, topic: TopicId, closed: bool) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            if closed {
                gateway.close_topic(&topic).await
            } else {
                gateway.reopen_topic(&topic).await
            }
        })
        .await
    }

    /// Marks the session as history, or makes it active again when `history` is false.
    pub async fn set_history(&self, session: SessionId, history: bool) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            if history {
                gateway.mark_history(&session).await
            } else {
                gateway.unmark_history(&session).await
            }
        })
        .await
    }

    pub async fn rename_topic(&self, topic: TopicId, name: String) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.rename_topic(&topic, &name).await })
            .await
    }

    /// Sets where the topic's sessions start, or clears it when there is none.
    pub async fn set_topic_directory(
        &self,
        topic: TopicId,
        directory: Option<Directory>,
    ) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.set_topic_directory(&topic, directory.as_ref()).await })
            .await
    }

    /// Moves the topic under `parent`, or to the top level when there is none.
    pub async fn move_topic(&self, topic: TopicId, parent: Option<TopicId>) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.move_topic(&topic, parent.as_ref()).await })
            .await
    }

    pub async fn merge_topic(&self, topic: TopicId, into: TopicId) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.merge_topic(&topic, &into).await })
            .await
    }

    /// Deletes the topic. Its subtopics move up a level first, since rh refuses to delete a topic
    /// that still has them; its sessions go back to Uncategorized.
    pub async fn delete_topic(&self, topic: TopicId) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            let topics = gateway.topics().await?;
            let above = topics
                .iter()
                .find(|known| known.id == topic)
                .and_then(|known| known.parent.clone());
            let subtopics = topics
                .iter()
                .filter(|known| known.parent.as_ref() == Some(&topic));
            futures::future::try_join_all(
                subtopics.map(|subtopic| gateway.move_topic(&subtopic.id, above.as_ref())),
            )
            .await?;
            gateway.delete_topic(&topic).await
        })
        .await
    }
}

use gateway::GatewayError;
use std::collections::HashMap;

use model::decisions::{Decision, Group, on_page};
use model::topics::{Placements, Suggestion, Topic};
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

    pub async fn topics(&self) -> Result<(Vec<Topic>, Placements), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            let topics = gateway.topics().await?;
            let filed =
                futures::future::try_join_all(topics.iter().map(|topic| gateway.topic_sessions(&topic.id)))
                    .await?;
            let placements = topics
                .iter()
                .zip(filed)
                .flat_map(|(topic, sessions)| sessions.into_iter().map(|session| (session, topic.id.clone())))
                .collect();
            Ok((topics, placements))
        })
        .await
    }

    pub async fn suggestions(
        &self,
        sessions: Vec<SessionId>,
    ) -> Result<HashMap<SessionId, Suggestion>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
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
            let topic = gateway.create_topic(&name).await?;
            gateway.place(&session, &topic.id).await?;
            Ok(topic)
        })
        .await
    }

    pub async fn delete_topic(&self, topic: TopicId) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.delete_topic(&topic).await })
            .await
    }
}

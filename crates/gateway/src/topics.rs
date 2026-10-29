use model::decisions::Decision;
use model::topics::{Suggestion, Topic};
use model::{SessionId, TopicId};
use wire::gateway::topics::{
    DecisionCreateRequest, DecisionSupersedeRequest, DecisionSuperseded, DecisionUpdate, PlacementReply,
    PlacementRequest, TopicCreateRequest, TopicDecision, TopicDecisionList, TopicList, TopicRecord,
    TopicSessionList,
};

use crate::{Gateway, GatewayError, translate};

fn topic(record: TopicRecord, last_active_ms: Option<i64>) -> Topic {
    Topic {
        id: TopicId::new(record.id),
        slug: record.slug,
        name: record.name,
        parent: record.parent.map(TopicId::new),
        last_active_ms,
    }
}

fn listed(list: TopicList) -> Vec<Topic> {
    list.topics
        .into_iter()
        .map(|summary| topic(summary.topic, summary.last_active.map(|at| at.timestamp_millis())))
        .collect()
}

impl Gateway {
    pub(crate) fn api_url(&self, segments: &[&str]) -> Result<reqwest::Url, GatewayError> {
        let mut url = reqwest::Url::parse(&self.base_url)
            .map_err(|err| GatewayError::Refused(format!("gateway url: {err}")))?;
        url.path_segments_mut()
            .map_err(|()| GatewayError::Refused("gateway url cannot take a path".into()))?
            .extend(segments);
        Ok(url)
    }

    pub async fn topics(&self) -> Result<Vec<Topic>, GatewayError> {
        let url = self.api_url(&["api", "topics"])?;
        Ok(listed(Self::read_json(self.http.get(url).send().await)?.await?))
    }

    pub async fn search_topics(&self, query: &str) -> Result<Vec<Topic>, GatewayError> {
        let mut url = self.api_url(&["api", "topics", "search"])?;
        url.query_pairs_mut().append_pair("q", query);
        Ok(listed(Self::read_json(self.http.get(url).send().await)?.await?))
    }

    pub async fn topic_sessions(&self, topic: &TopicId) -> Result<Vec<SessionId>, GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str(), "sessions"])?;
        let list: TopicSessionList = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(list
            .sessions
            .iter()
            .map(|placed| translate::session(&placed.placement.session_id))
            .collect())
    }

    pub async fn create_topic(&self, name: &str) -> Result<Topic, GatewayError> {
        let url = self.api_url(&["api", "topics"])?;
        let body = TopicCreateRequest {
            name: name.to_owned(),
            aliases: Vec::new(),
            parent: None,
            directory: None,
        };
        let created: TopicRecord = Self::read_json(self.http.post(url).json(&body).send().await)?.await?;
        Ok(topic(created, None))
    }

    pub async fn suggestion(&self, session: &SessionId) -> Result<Option<Suggestion>, GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let url = self.api_url(&["api", "sessions", &segment, "placement"])?;
        let reply: PlacementReply = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(reply.suggestion.map(|suggestion| Suggestion {
            name: suggestion.name,
            topic: suggestion.topic_id.map(TopicId::new),
        }))
    }

    pub async fn place(&self, session: &SessionId, topic: &TopicId) -> Result<(), GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let url = self.api_url(&["api", "sessions", &segment, "placement"])?;
        let body = PlacementRequest {
            topic_id: topic.as_str().to_owned(),
        };
        let _: PlacementReply = Self::read_json(self.http.put(url).json(&body).send().await)?.await?;
        Ok(())
    }

    pub async fn unplace(&self, session: &SessionId) -> Result<(), GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let url = self.api_url(&["api", "sessions", &segment, "placement"])?;
        let _: PlacementReply = Self::read_json(self.http.delete(url).send().await)?.await?;
        Ok(())
    }

    /// The topic's own decisions, oldest first.
    pub async fn decisions(&self, topic: &TopicId) -> Result<Vec<Decision>, GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str(), "decisions"])?;
        let list: TopicDecisionList = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(list.decisions.into_iter().map(translate::decision).collect())
    }

    pub async fn add_decision(&self, topic: &TopicId, text: &str) -> Result<Decision, GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str(), "decisions"])?;
        let body = DecisionCreateRequest {
            text: text.to_owned(),
            session_id: None,
        };
        let added: TopicDecision = Self::decided(self.http.post(url).json(&body).send().await).await?;
        Ok(translate::decision(added))
    }

    pub async fn edit_decision(
        &self,
        topic: &TopicId,
        decision: &str,
        text: &str,
    ) -> Result<Decision, GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str(), "decisions", decision])?;
        let body = DecisionUpdate {
            text: text.to_owned(),
        };
        let edited: TopicDecision = Self::decided(self.http.patch(url).json(&body).send().await).await?;
        Ok(translate::decision(edited))
    }

    /// Marks a decision superseded and, given `text`, adds the decision that replaces it.
    pub async fn supersede_decision(
        &self,
        topic: &TopicId,
        decision: &str,
        text: Option<&str>,
    ) -> Result<(), GatewayError> {
        let url = self.api_url(&[
            "api",
            "topics",
            topic.as_str(),
            "decisions",
            decision,
            "supersede",
        ])?;
        let body = DecisionSupersedeRequest {
            text: text.map(str::to_owned),
            session_id: None,
        };
        let _: DecisionSuperseded = Self::decided(self.http.post(url).json(&body).send().await).await?;
        Ok(())
    }

    /// A decision write's reply. rh answers 409 for a decision already superseded and 400 for empty
    /// text, and both come back as `Conflict` carrying rh's own message.
    async fn decided<T: serde::de::DeserializeOwned>(
        sent: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<T, GatewayError> {
        match Self::read_json(sent)?.await {
            Err(GatewayError::Status {
                status: 400 | 409,
                detail,
            }) => Err(GatewayError::Conflict(rh_message(detail))),
            read => read,
        }
    }

    /// Deletes a topic; its sessions go back to Uncategorized. rh refuses category topics and topics
    /// with child topics, and that refusal comes back as `Conflict` carrying rh's own message.
    pub async fn delete_topic(&self, topic: &TopicId) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str()])?;
        let reply = self.http.delete(url).send().await.map_err(GatewayError::Http)?;
        match Self::check(reply).await {
            Err(GatewayError::Status { status: 409, detail }) => {
                Err(GatewayError::Conflict(rh_message(detail)))
            }
            checked => checked.map(drop),
        }
    }
}

/// The message in rh's error body, or the body itself when it isn't one.
fn rh_message(detail: String) -> String {
    serde_json::from_str::<wire::gateway::GatewayError>(&detail).map_or(detail, |body| body.error)
}

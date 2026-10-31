use std::collections::HashMap;

use model::decisions::Decision;
use model::entities::{Catalog, Entity};
use model::preview::{StartupPart, StartupPreview};
use model::topics::{
    Directory, FiledBy, FiledConversation, FiledSession, HistoryMarks, Suggestion, Topic, TopicFiling,
};
use model::visibility::{SessionVisibility, Visibility};
use model::{SessionId, TopicId};
use wire::gateway::topics::{
    ConversationPlacementReply, DecisionCreateRequest, DecisionSupersedeRequest, DecisionSuperseded,
    DecisionUpdate, PlacedBy, PlacementReply, PlacementRequest, SessionHistoryList, SessionHistoryMark,
    SessionSuggestionList, StartupContext, TopicConversation, TopicCreateRequest, TopicDecision,
    TopicDecisionList, TopicDirectory, TopicKind, TopicList, TopicMergeRequest, TopicRecord, TopicSession,
    TopicSessionList, TopicSuggestion, TopicUpdate,
};

use crate::{Gateway, GatewayError, translate};

fn topic(record: TopicRecord, last_active_ms: Option<i64>) -> Topic {
    Topic {
        id: TopicId::new(record.id),
        slug: record.slug,
        name: record.name,
        parent: record.parent.map(TopicId::new),
        last_active_ms,
        updated_ms: record.updated_at.timestamp_millis(),
        closed_ms: record.closed_at.map(|at| at.timestamp_millis()),
        directory: record.directory.map(|directory| match directory {
            TopicDirectory::Local { node, path } => Directory::Local { node, path },
            TopicDirectory::Shared { path } => Directory::Shared { path },
        }),
        owner: record.owner,
        visibility: record.visibility.as_deref().map(Visibility::parse),
    }
}

fn listed(list: TopicList) -> Vec<Topic> {
    list.topics
        .into_iter()
        .map(|summary| topic(summary.topic, summary.last_active.map(|at| at.timestamp_millis())))
        .collect()
}

fn filing(sessions: Vec<TopicSession>, conversations: Vec<TopicConversation>) -> TopicFiling {
    TopicFiling {
        sessions: sessions
            .into_iter()
            .map(|placed| FiledSession {
                session: translate::session(&placed.placement.session_id),
                topic: TopicId::new(placed.placement.topic_id),
                title: placed.title,
                last_active_ms: placed.last_active.map(|at| at.timestamp_millis()),
                by: filed_by(placed.placement.by),
                node: placed.node,
                visibility: placed.visibility.as_deref().map(|effective| SessionVisibility {
                    effective: Visibility::parse(effective),
                    marked: placed.visibility_mark.is_some(),
                }),
            })
            .collect(),
        conversations: conversations
            .into_iter()
            .map(|filed| FiledConversation {
                id: filed.placement.conversation_id,
                topic: TopicId::new(filed.placement.topic_id),
                title: filed.title,
                channel: filed.channel,
                last_active_ms: filed.last_active.map(|at| at.timestamp_millis()),
                by: filed_by(filed.placement.by),
                node: filed.node,
            })
            .collect(),
    }
}

fn marks(history: &[SessionHistoryMark]) -> HistoryMarks {
    history
        .iter()
        .map(|mark| {
            (
                translate::session(&mark.session_id),
                mark.marked_at.timestamp_millis(),
            )
        })
        .collect()
}

fn suggestion(suggested: TopicSuggestion) -> Suggestion {
    Suggestion {
        name: suggested.name,
        topic: suggested.topic_id.map(TopicId::new),
    }
}

fn filed_by(by: PlacedBy) -> FiledBy {
    match by {
        PlacedBy::You => FiledBy::You,
        PlacedBy::Categorizer => FiledBy::Categorizer,
    }
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

    pub async fn topic_sessions(&self, topic: &TopicId) -> Result<TopicFiling, GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str(), "sessions"])?;
        let list: TopicSessionList = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(filing(list.sessions, list.conversations))
    }

    /// Every topic's filed sessions and conversations and the history marks, in one request; `None`
    /// from an rh older than the route, which is read a topic at a time instead.
    pub async fn filing(&self) -> Result<Option<(TopicFiling, HistoryMarks)>, GatewayError> {
        let url = self.api_url(&["api", "topics", "filing"])?;
        match Self::read_json::<wire::gateway::topics::TopicFiling>(self.http.get(url).send().await)?.await {
            Ok(all) => Ok(Some((
                filing(all.sessions, all.conversations),
                marks(&all.history),
            ))),
            Err(err) if err.is_missing_route() => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// The entity types and every entity filed under them.
    pub async fn catalog(&self) -> Result<Catalog, GatewayError> {
        let (types, entities) =
            futures::future::try_join(self.of_kind("type"), self.of_kind("entity")).await?;
        Ok(Catalog {
            types: types.into_iter().map(|(kind, _)| kind).collect(),
            entities: entities
                .into_iter()
                .map(|(topic, aliases)| Entity { topic, aliases })
                .collect(),
        })
    }

    async fn of_kind(&self, kind: &str) -> Result<Vec<(Topic, Vec<String>)>, GatewayError> {
        let mut url = self.api_url(&["api", "topics"])?;
        url.query_pairs_mut().append_pair("kind", kind);
        let list: TopicList = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(list
            .topics
            .into_iter()
            .map(|summary| {
                let aliases = summary.topic.aliases.clone();
                (topic(summary.topic, None), aliases)
            })
            .collect())
    }

    /// Creates a topic, under `parent` when given. rh's refusals (a name taken, too deep) come back
    /// as `Conflict` carrying rh's own message.
    pub async fn create_topic(&self, name: &str, parent: Option<&TopicId>) -> Result<Topic, GatewayError> {
        self.create(TopicKind::Work, name, parent).await
    }

    /// Creates an entity type, under `parent` when given.
    pub async fn create_type(&self, name: &str, parent: Option<&TopicId>) -> Result<Topic, GatewayError> {
        self.create(TopicKind::Type, name, parent).await
    }

    async fn create(
        &self,
        kind: TopicKind,
        name: &str,
        parent: Option<&TopicId>,
    ) -> Result<Topic, GatewayError> {
        let url = self.api_url(&["api", "topics"])?;
        let body = TopicCreateRequest {
            kind,
            name: name.to_owned(),
            aliases: Vec::new(),
            parent: parent.map(|parent| parent.as_str().to_owned()),
            directory: None,
        };
        let created: TopicRecord = Self::refusable(self.http.post(url).json(&body).send().await).await?;
        Ok(topic(created, None))
    }

    pub async fn suggestion(&self, session: &SessionId) -> Result<Option<Suggestion>, GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let url = self.api_url(&["api", "sessions", &segment, "placement"])?;
        let reply: PlacementReply = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(reply.suggestion.map(suggestion))
    }

    /// Where every unplaced session might go, in one request; `None` from an rh older than the
    /// route, which is asked a session at a time instead.
    pub async fn suggestions(&self) -> Result<Option<HashMap<SessionId, Suggestion>>, GatewayError> {
        let url = self.api_url(&["api", "sessions", "suggestions"])?;
        match Self::read_json::<SessionSuggestionList>(self.http.get(url).send().await)?.await {
            Ok(list) => Ok(Some(
                list.sessions
                    .into_iter()
                    .map(|suggested| {
                        (
                            translate::session(&suggested.session_id),
                            suggestion(suggested.suggestion),
                        )
                    })
                    .collect(),
            )),
            Err(err) if err.is_missing_route() => Ok(None),
            Err(err) => Err(err),
        }
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

    pub async fn rename_topic(&self, topic: &TopicId, name: &str) -> Result<(), GatewayError> {
        self.update_topic(
            topic,
            TopicUpdate {
                name: Some(name.to_owned()),
                ..TopicUpdate::default()
            },
        )
        .await
    }

    /// Replaces the other names a topic or entity goes by.
    pub async fn set_aliases(&self, topic: &TopicId, aliases: Vec<String>) -> Result<(), GatewayError> {
        self.update_topic(
            topic,
            TopicUpdate {
                aliases: Some(aliases),
                ..TopicUpdate::default()
            },
        )
        .await
    }

    /// Takes a conversation back out of its topic.
    pub async fn unfile_conversation(&self, conversation: &str) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "conversations", conversation, "placement"])?;
        let _: ConversationPlacementReply = Self::read_json(self.http.delete(url).send().await)?.await?;
        Ok(())
    }

    /// Moves a topic under `parent`, or to the top level when there is none, within the four
    /// levels rh allows; an entity moves only under a type.
    pub async fn move_topic(&self, topic: &TopicId, parent: Option<&TopicId>) -> Result<(), GatewayError> {
        self.update_topic(
            topic,
            TopicUpdate {
                parent: Some(parent.map(|parent| parent.as_str().to_owned())),
                ..TopicUpdate::default()
            },
        )
        .await
    }

    async fn update_topic(&self, topic: &TopicId, update: TopicUpdate) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str()])?;
        let _: TopicRecord = Self::refusable(self.http.patch(url).json(&update).send().await).await?;
        Ok(())
    }

    /// Merges a topic into another: its sessions, subtopics and names move there, and its old
    /// address redirects to it.
    pub async fn merge_topic(&self, topic: &TopicId, into: &TopicId) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str(), "merge"])?;
        let body = TopicMergeRequest {
            into: into.as_str().to_owned(),
        };
        let _: TopicRecord = Self::refusable(self.http.post(url).json(&body).send().await).await?;
        Ok(())
    }

    /// Closes a topic you're done with; it is past until one of its sessions is active again.
    pub async fn close_topic(&self, topic: &TopicId) -> Result<(), GatewayError> {
        self.post_topic(topic, "close").await
    }

    pub async fn reopen_topic(&self, topic: &TopicId) -> Result<(), GatewayError> {
        self.post_topic(topic, "reopen").await
    }

    async fn post_topic(&self, topic: &TopicId, verb: &str) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str(), verb])?;
        let _: TopicRecord = Self::read_json(self.http.post(url).send().await)?.await?;
        Ok(())
    }

    /// When each session was marked as history.
    pub async fn history_marks(&self) -> Result<HistoryMarks, GatewayError> {
        let url = self.api_url(&["api", "sessions", "history"])?;
        let list: SessionHistoryList = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(marks(&list.sessions))
    }

    /// Marks a session as history: it leaves its topic's active sessions until it is active again.
    pub async fn mark_history(&self, session: &SessionId) -> Result<(), GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let url = self.api_url(&["api", "sessions", &segment, "history"])?;
        let _: SessionHistoryMark = Self::read_json(self.http.put(url).send().await)?.await?;
        Ok(())
    }

    pub async fn unmark_history(&self, session: &SessionId) -> Result<(), GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let url = self.api_url(&["api", "sessions", &segment, "history"])?;
        Self::check(self.http.delete(url).send().await.map_err(GatewayError::Http)?)
            .await
            .map(drop)
    }

    /// What a session started in the topic right now would be told; rh answers 503 when startup
    /// memory isn't set up on the node.
    pub async fn topic_startup(&self, topic: &TopicId) -> Result<StartupPreview, GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str(), "startup"])?;
        let context: StartupContext = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(StartupPreview {
            sections: context
                .sections
                .into_iter()
                .map(|section| StartupPart {
                    title: section.title,
                    text: section.text,
                })
                .collect(),
            tokens: context.tokens,
        })
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
        let added: TopicDecision = Self::refusable(self.http.post(url).json(&body).send().await).await?;
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
        let edited: TopicDecision = Self::refusable(self.http.patch(url).json(&body).send().await).await?;
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
        let _: DecisionSuperseded = Self::refusable(self.http.post(url).json(&body).send().await).await?;
        Ok(())
    }

    /// A topic or decision write's reply. rh's refusals (400 and 409: a name already taken, a
    /// subtopic given subtopics, a decision already superseded, empty text) come back as
    /// `Conflict` carrying rh's own message.
    pub(crate) async fn refusable<T: serde::de::DeserializeOwned>(
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

    /// Deletes a topic, type or entity; a topic's sessions go back to Uncategorized. rh refuses one
    /// that still has subtopics, and that refusal comes back as `Conflict` carrying rh's own message.
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
pub(crate) fn rh_message(detail: String) -> String {
    serde_json::from_str::<wire::gateway::GatewayError>(&detail).map_or(detail, |body| body.error)
}

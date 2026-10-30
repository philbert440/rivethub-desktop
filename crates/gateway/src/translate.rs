mod stream;

use model::{
    HarnessId, NativeSessionId, SessionId, TaskId,
    session::{SessionStatus, SessionSummary},
};
use wire::harness::{HarnessSessionStatus, HarnessSessionSummary};

use crate::GatewayError;
pub use stream::{Running, event};

pub fn session(id: &wire::session_id::SessionId) -> SessionId {
    let raw = id.as_str();
    let (harness, native) = raw.split_once(':').unwrap_or(("", raw));
    SessionId::new(HarnessId::new(harness), NativeSessionId::new(native))
}

pub fn session_status(status: HarnessSessionStatus) -> SessionStatus {
    match status {
        HarnessSessionStatus::Active => SessionStatus::Active,
        HarnessSessionStatus::Idle => SessionStatus::Idle,
        HarnessSessionStatus::Ended => SessionStatus::Ended,
        HarnessSessionStatus::Error => SessionStatus::Failed,
    }
}

pub fn roster_command(id: &SessionId) -> Result<String, GatewayError> {
    let harness: wire::harness::HarnessId = id
        .harness()
        .as_str()
        .parse()
        .map_err(|_| GatewayError::SessionId(id.to_string()))?;
    let command = serde_json::to_value(wire::harness::RosterCommand::from(harness))
        .map_err(|_| GatewayError::SessionId(id.to_string()))?;
    command
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| GatewayError::SessionId(id.to_string()))
}

pub fn agent(raw: wire::gateway::agents::AgentPreset) -> model::agents::Agent {
    let filled = |value: String| Some(value).filter(|value| !value.trim().is_empty());
    model::agents::Agent {
        id: raw.id,
        name: raw.name,
        color: model::agents::parse_color(&raw.color),
        harness: raw.harness_id.map(|harness| HarnessId::new(harness.to_string())),
        model: filled(raw.model),
        node: raw.node,
        effort: filled(raw.effort),
        directory: raw.directory,
        instructions: filled(raw.system_prompt),
    }
}

pub fn roster(raw: wire::gateway::agents::AgentListResponse) -> model::agents::Roster {
    let agents = raw.agents.into_iter().map(agent).collect();
    model::agents::Roster {
        node: raw.node,
        agents,
        storage: model::agents::AgentStorage {
            shared_presets: raw
                .backend
                .map(|backend| backend == wire::gateway::agents::AgentStoreBackend::Postgres),
            folder_root: raw.directory_root,
            shared_folder: raw.shared_dir,
        },
    }
}

pub fn hits(raw: wire::gateway::memory::MemorySearchResponse) -> Vec<model::search::Hit> {
    raw.results
        .into_iter()
        .filter_map(|hit| {
            let id = hit.session_id?;
            let parsed: wire::session_id::SessionId = id.parse().ok()?;
            Some(model::search::Hit {
                session: session(&parsed),
                text: hit.content,
                at_ms: hit.created_at.timestamp_millis(),
            })
        })
        .collect()
}

pub fn attachment(raw: wire::gateway::uploads::UploadCreated) -> model::prompt::Attachment {
    model::prompt::Attachment {
        name: raw.name,
        media_type: raw.media_type,
        uri: raw.uri,
    }
}

fn attachments(prompt: &model::prompt::Prompt) -> Option<Vec<wire::gateway::agents::UserTurnAttachment>> {
    let attachments = prompt
        .attachments
        .iter()
        .map(|attachment| wire::gateway::agents::UserTurnAttachment {
            mime: attachment.media_type.clone(),
            path_or_uri: attachment.uri.clone(),
            name: Some(attachment.name.clone()),
        })
        .collect::<Vec<_>>();
    Some(attachments).filter(|list| !list.is_empty())
}

pub fn steer(prompt: &model::prompt::Prompt) -> wire::gateway::sessions::SessionSteerRequest {
    wire::gateway::sessions::SessionSteerRequest {
        text: prompt.text.clone(),
        attachments: attachments(prompt),
    }
}

pub fn prompt(
    prompt: &model::prompt::Prompt,
) -> Result<wire::gateway::sessions::SessionPostRequest, GatewayError> {
    let thinking = prompt
        .switch
        .effort
        .as_deref()
        .map(|effort| {
            effort
                .parse::<wire::stream::ThinkingLevel>()
                .map_err(|_| GatewayError::Refused(format!("{effort} is not an effort rh knows")))
        })
        .transpose()?;
    Ok(wire::gateway::sessions::SessionPostRequest {
        exclude_tools: None,
        text: prompt.text.clone(),
        user_id: None,
        agent: None,
        thinking,
        system_prompt: None,
        attachments: attachments(prompt),
        harness: None,
        model: prompt.switch.model.clone(),
        working_dir: None,
    })
}

pub fn found(raw: wire::gateway::memory::MemorySearchResponse) -> Vec<model::memory::Found> {
    use wire::gateway::memory::MemoryHitSource;
    raw.results
        .into_iter()
        .map(|hit| model::memory::Found {
            source: match hit.source {
                MemoryHitSource::Message => model::memory::Source::Message,
                MemoryHitSource::Summary => model::memory::Source::Summary,
            },
            session: hit
                .session_id
                .and_then(|id| id.parse::<wire::session_id::SessionId>().ok())
                .map(|id| session(&id)),
            text: hit.content,
            at_ms: hit.created_at.timestamp_millis(),
        })
        .collect()
}

pub fn session_memory(raw: wire::gateway::memory::MemorySummaryDagResponse) -> model::memory::SessionMemory {
    use model::memory::{MessageTally, SessionMemory, Summary, SummaryKind, Vector};
    use wire::gateway::memory::{SummaryKind as Kind, VectorState};
    let vector = |state| match state {
        VectorState::Embedded => Vector::Embedded,
        VectorState::Pending => Vector::Pending,
        VectorState::Failed => Vector::Failed,
        VectorState::Skipped => Vector::Skipped,
    };
    let tally = raw.messages;
    SessionMemory {
        summaries: raw
            .summaries
            .into_iter()
            .map(|node| Summary {
                id: node.id,
                parent: node.parent_id,
                kind: match node.kind {
                    Kind::Leaf => SummaryKind::Leaf,
                    Kind::Branch => SummaryKind::Branch,
                    Kind::Root => SummaryKind::Root,
                },
                text: node.content,
                messages: node.message_count,
                from_ms: node.earliest_at.map(|at| at.timestamp_millis()),
                to_ms: node.latest_at.map(|at| at.timestamp_millis()),
                vector: vector(node.vector),
            })
            .collect(),
        messages: MessageTally {
            total: tally.total,
            user: tally.user,
            assistant: tally.assistant,
            system: tally.system,
            tool: tally.tool,
            embedded: tally.embedded,
            pending: tally.pending,
            failed: tally.failed,
            skipped: tally.skipped,
        },
    }
}

pub fn browse_window(period: model::memory::Period) -> Option<wire::gateway::memory::BrowseWindow> {
    use model::memory::Period;
    use wire::gateway::memory::BrowseWindow;
    match period {
        Period::AnyTime => None,
        Period::Today => Some(BrowseWindow::Today),
        Period::Yesterday => Some(BrowseWindow::Yesterday),
        Period::LastWeek => Some(BrowseWindow::Last7d),
        Period::LastTwoWeeks => Some(BrowseWindow::Last14d),
    }
}

pub fn page(raw: wire::gateway::wiki::WikiPageResponse) -> model::wiki::Page {
    model::wiki::Page {
        slug: raw.slug,
        title: raw.title,
        topic: raw.topic_id.map(model::TopicId::new),
        tags: raw.tags,
        updated_ms: raw.updated_at.timestamp_millis(),
        markdown: raw.markdown,
        current_state: raw.current_state,
        article: raw.article,
        citations: raw
            .citations
            .into_iter()
            .map(|citation| model::wiki::Citation {
                summary: citation.summary_id,
                at_ms: citation.date.map(|date| date.timestamp_millis()),
                conversation: citation.conversation_id,
                session: citation.session_id.as_ref().map(session),
                title: citation.title,
                channel: citation.channel,
                topic: citation.topic_id.map(model::TopicId::new),
            })
            .collect(),
    }
}

pub fn switches(raw: Option<wire::gateway::sessions::SessionSwitches>) -> model::session::Switches {
    raw.map_or_else(model::session::Switches::default, |raw| {
        model::session::Switches {
            model: raw.model,
            effort: raw.effort,
        }
    })
}

pub fn decision(raw: wire::gateway::topics::TopicDecision) -> model::decisions::Decision {
    use model::decisions::Status;
    use wire::gateway::topics::DecisionStatus;
    model::decisions::Decision {
        id: raw.id,
        topic: model::TopicId::new(raw.topic_id),
        text: raw.text,
        status: match raw.status {
            DecisionStatus::Active => Status::Active,
            DecisionStatus::Superseded => Status::Superseded,
        },
        by: raw.by,
        updated_ms: raw.updated_at.timestamp_millis(),
    }
}

pub fn listing(raw: wire::gateway::wiki::WikiTopic) -> model::wiki::Listing {
    model::wiki::Listing {
        slug: raw.slug,
        title: raw.title,
        updated_ms: raw.updated_at.timestamp_millis(),
        owner: raw.topic_id.map(model::TopicId::new),
    }
}

pub fn tag_proposal(raw: wire::gateway::memory::PendingTagWire) -> model::memory::TagProposal {
    use model::memory::TagChange;
    use wire::gateway::memory::PendingTagAction;
    model::memory::TagProposal {
        tag: format!("{}:{}", raw.tag.key, raw.tag.value),
        change: match raw.action {
            Some(PendingTagAction::Remove) => TagChange::Remove,
            Some(PendingTagAction::Add) | None => TagChange::Add,
        },
        conversation: raw
            .title
            .or(raw.session_key)
            .unwrap_or_else(|| raw.tag.entity_id.clone()),
        reason: raw.tag.reason,
        id: raw.tag.id,
    }
}

pub fn term_proposal(raw: wire::gateway::memory::TagTaxonomyEntry) -> model::memory::TermProposal {
    model::memory::TermProposal {
        key: raw.key,
        value: raw.value,
        display: raw.display,
        reason: raw.reason,
    }
}

pub fn verdict(verdict: model::memory::Verdict) -> wire::gateway::memory::TagState {
    match verdict {
        model::memory::Verdict::Accept => wire::gateway::memory::TagState::Accepted,
        model::memory::Verdict::Reject => wire::gateway::memory::TagState::Rejected,
    }
}

pub fn wire_session(id: &SessionId) -> Result<wire::session_id::SessionId, GatewayError> {
    id.to_string()
        .parse()
        .map_err(|_| GatewayError::SessionId(id.to_string()))
}

pub fn offer(raw: wire::harness::HarnessOffering) -> model::harness::HarnessOffer {
    let capabilities = raw.capabilities;
    model::harness::HarnessOffer {
        id: HarnessId::new(raw.harness_id.to_string()),
        chat: raw.chat,
        lists_sessions: capabilities.observation.list_sessions,
        takes_images: capabilities.image_attachments.unwrap_or(false),
        steers: capabilities.control.steering.unwrap_or(false),
        models: capabilities
            .models
            .unwrap_or_default()
            .into_iter()
            .map(|choice| model::harness::ModelChoice {
                id: choice.id,
                label: choice.label,
                default: choice.is_default.unwrap_or(false),
            })
            .collect(),
    }
}

pub fn memory_health(
    stats: &wire::gateway::memory::MemoryStatsResponse,
    health: &wire::gateway::memory::MemoryHealthResponse,
) -> model::health::MemoryHealth {
    use wire::gateway::memory::{EmbedStatus, MemoryHealthStatus};
    model::health::MemoryHealth {
        status: match health.status {
            MemoryHealthStatus::Ok => model::health::Status::Ok,
            MemoryHealthStatus::Degraded => model::health::Status::Degraded,
            MemoryHealthStatus::Error => model::health::Status::Error,
        },
        semantic: health.embeddings.status == EmbedStatus::Ok,
        conversations: stats.conversations,
        messages: stats.messages,
    }
}

pub fn summary(raw: HarnessSessionSummary) -> SessionSummary {
    SessionSummary {
        id: session(&raw.session_id),
        title: raw.title,
        status: session_status(raw.status),
        model: raw.model,
        effort: raw.effort,
        cwd: raw.cwd,
        updated_at_ms: raw.updated_at.timestamp_millis(),
        parent: raw.parent_session_id.as_ref().map(session),
        task: raw.task_id.map(TaskId::new),
        node: None,
        blocked: raw.blocked.unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_effort_the_composer_offers_is_one_rh_takes() {
        for effort in model::agents::EFFORTS {
            assert!(
                effort.parse::<wire::stream::ThinkingLevel>().is_ok(),
                "rh has no {effort} effort"
            );
        }
    }
}

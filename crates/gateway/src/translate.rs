use model::{
    HarnessId, NativeSessionId, SessionId,
    session::{
        Activity, Approval, ContextUsage, Role, SessionEvent, SessionStatus, SessionSummary, ToolCall,
        ToolState, Turn,
    },
};
use wire::gateway::sessions::{
    HarnessTranscriptTurn, SessionMessageRole, TranscriptBlock, TranscriptToolStatus,
};
use wire::harness::{HarnessEvent, HarnessSessionStatus, HarnessSessionSummary, HarnessStatus};

use crate::GatewayError;

pub fn session(id: &wire::session_id::SessionId) -> SessionId {
    let raw = id.as_str();
    let (harness, native) = raw.split_once(':').unwrap_or(("", raw));
    SessionId::new(HarnessId::new(harness), NativeSessionId::new(native))
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

pub fn roster(raw: wire::gateway::agents::AgentListResponse) -> model::agents::Roster {
    let agents = raw
        .agents
        .into_iter()
        .map(|agent| model::agents::Agent {
            id: agent.id,
            name: agent.name,
            color: model::agents::parse_color(&agent.color),
            harness: agent
                .harness_id
                .map(|harness| HarnessId::new(harness.to_string())),
            model: Some(agent.model).filter(|model| !model.is_empty()),
            node: agent.node,
        })
        .collect();
    model::agents::Roster {
        node: raw.node,
        agents,
    }
}

pub fn hits(raw: wire::gateway::memory::MemorySearchResponse) -> Vec<model::search::Hit> {
    use wire::message::Role as HitRole;
    raw.results
        .into_iter()
        .filter(|hit| !matches!(hit.role, Some(HitRole::Tool | HitRole::System)))
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

pub fn found(raw: wire::gateway::memory::MemorySearchResponse) -> Vec<model::memory::Found> {
    use wire::gateway::memory::MemoryHitSource;
    use wire::message::Role as HitRole;
    raw.results
        .into_iter()
        .filter(|hit| !matches!(hit.role, Some(HitRole::Tool | HitRole::System)))
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
    }
}

pub fn listing(raw: wire::gateway::wiki::WikiTopic) -> model::wiki::Listing {
    model::wiki::Listing {
        slug: raw.slug,
        title: raw.title,
        updated_ms: raw.updated_at.timestamp_millis(),
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

pub fn summary(raw: HarnessSessionSummary) -> SessionSummary {
    SessionSummary {
        id: session(&raw.session_id),
        title: raw.title,
        status: match raw.status {
            HarnessSessionStatus::Active => SessionStatus::Active,
            HarnessSessionStatus::Idle => SessionStatus::Idle,
            HarnessSessionStatus::Ended => SessionStatus::Ended,
            HarnessSessionStatus::Error => SessionStatus::Failed,
        },
        model: raw.model,
        cwd: raw.cwd,
        updated_at_ms: raw.updated_at.timestamp_millis(),
        parent: raw.parent_session_id.as_ref().map(session),
        node: None,
        blocked: raw.blocked.unwrap_or(false),
    }
}

fn turn(raw: HarnessTranscriptTurn) -> Turn {
    let ends_with_text = raw.last_block.is_none_or(|block| block == TranscriptBlock::Text);
    Turn {
        ends_with_text,
        role: match raw.role {
            SessionMessageRole::User => Role::User,
            SessionMessageRole::Assistant => Role::Assistant,
        },
        text: raw.text,
        thinking: raw.thinking.unwrap_or_default(),
        tools: raw
            .tools
            .unwrap_or_default()
            .into_iter()
            .map(|tool| {
                let state = match tool.status {
                    TranscriptToolStatus::Running => ToolState::Running,
                    TranscriptToolStatus::Done => ToolState::Done,
                    TranscriptToolStatus::Error => ToolState::Failed,
                };
                let input = crate::tool_input::parse(&tool.name, tool.input.as_ref().or(tool.args.as_ref()));
                let output = tool
                    .result_text
                    .as_deref()
                    .map(|text| crate::tool_input::output(&tool.name, text, state == ToolState::Failed));
                ToolCall {
                    id: tool.id,
                    name: tool.name,
                    state,
                    input,
                    output,
                }
            })
            .collect(),
    }
}

fn context(turns: &[HarnessTranscriptTurn], window: Option<u64>) -> Option<ContextUsage> {
    let window = window?;
    let used = turns
        .iter()
        .rev()
        .find_map(|turn| turn.usage.as_ref())
        .map(|usage| usage.prompt_tokens + usage.completion_tokens)?;
    Some(ContextUsage { used, window })
}

pub fn event(raw: HarnessEvent) -> Option<SessionEvent> {
    Some(match raw {
        HarnessEvent::AssistantDelta { text, .. } => SessionEvent::AssistantText(text),
        HarnessEvent::ReasoningDelta { text, .. } => SessionEvent::Reasoning(text),
        HarnessEvent::ToolUse {
            tool_call_id,
            name,
            input,
            ..
        } => SessionEvent::ToolStarted {
            id: tool_call_id,
            input: crate::tool_input::parse(&name, Some(&input)),
            name,
        },
        HarnessEvent::ToolResult {
            tool_call_id,
            name,
            output,
            is_error,
            ..
        } => {
            let failed = is_error.unwrap_or(false);
            SessionEvent::ToolFinished {
                id: tool_call_id,
                failed,
                output: crate::tool_input::output(&name, &output, failed),
            }
        }
        HarnessEvent::ApprovalRequest {
            request_id,
            name,
            reason,
            ..
        } => SessionEvent::ApprovalRequested(Approval {
            request_id,
            tool: name,
            reason,
        }),
        HarnessEvent::ApprovalResolved { request_id, .. } => SessionEvent::ApprovalResolved { request_id },
        HarnessEvent::TurnComplete { .. } => SessionEvent::TurnComplete,
        HarnessEvent::Error { message, .. } => SessionEvent::Error(message),
        HarnessEvent::Transcript {
            turns,
            context_window,
            ..
        } => {
            let usage = context(&turns, context_window);
            SessionEvent::Snapshot {
                turns: turns.into_iter().map(turn).collect(),
                context: usage,
            }
        }
        HarnessEvent::Status(frame) => SessionEvent::Activity(match frame.status {
            HarnessStatus::Working => Activity::Working,
            HarnessStatus::Blocked => Activity::Blocked,
            HarnessStatus::Idle => Activity::Idle,
        }),
        HarnessEvent::SessionCreated { .. }
        | HarnessEvent::SessionUpdated { .. }
        | HarnessEvent::Prompt { .. } => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_counts_cached_prompt_tokens_once() {
        let turn: HarnessTranscriptTurn = serde_json::from_str(
            r#"{"role":"assistant","text":"hi","usage":{"promptTokens":15,"completionTokens":3,"cachedTokens":5}}"#,
        )
        .unwrap_or_else(|err| unreachable!("fixture: {err}"));
        assert_eq!(context(&[turn], Some(100)).map(|usage| usage.used), Some(18));
    }
}

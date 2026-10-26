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

pub fn wire_session(id: &SessionId) -> Result<wire::session_id::SessionId, GatewayError> {
    id.to_string()
        .parse()
        .map_err(|_| GatewayError::SessionId(id.to_string()))
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
            .map(|tool| ToolCall {
                id: tool.id,
                name: tool.name,
                state: match tool.status {
                    TranscriptToolStatus::Running => ToolState::Running,
                    TranscriptToolStatus::Done => ToolState::Done,
                    TranscriptToolStatus::Error => ToolState::Failed,
                },
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
        .map(|usage| usage.prompt_tokens + usage.cached_tokens + usage.completion_tokens)?;
    Some(ContextUsage { used, window })
}

pub fn event(raw: HarnessEvent) -> Option<SessionEvent> {
    Some(match raw {
        HarnessEvent::AssistantDelta { text, .. } => SessionEvent::AssistantText(text),
        HarnessEvent::ReasoningDelta { text, .. } => SessionEvent::Reasoning(text),
        HarnessEvent::ToolUse {
            tool_call_id, name, ..
        } => SessionEvent::ToolStarted {
            id: tool_call_id,
            name,
        },
        HarnessEvent::ToolResult {
            tool_call_id,
            is_error,
            ..
        } => SessionEvent::ToolFinished {
            id: tool_call_id,
            failed: is_error.unwrap_or(false),
        },
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

use std::collections::HashMap;

use model::{
    plan::{Plan, Step, StepStatus},
    session::{
        Activity, Approval, ContextUsage, Driver, Driving, Role, SessionEvent, TerminalApproval, ToolCall,
        ToolState, Turn,
    },
    usage::Usage,
};
use wire::gateway::sessions::{
    HarnessTranscriptTurn, MessageUsage, SessionMessageRole, TranscriptBlock, TranscriptToolStatus,
};
use wire::harness::{HarnessEvent, HarnessStatus, SessionDriver};

fn plan(entries: Vec<wire::harness::PlanEntry>) -> Plan {
    Plan {
        steps: entries
            .into_iter()
            .map(|entry| Step {
                id: None,
                content: entry.content,
                status: match entry.status {
                    wire::harness::PlanStatus::Pending => StepStatus::Pending,
                    wire::harness::PlanStatus::InProgress => StepStatus::InProgress,
                    wire::harness::PlanStatus::Completed => StepStatus::Completed,
                },
            })
            .collect(),
    }
}

/// What a turn reports as it ends. rh counts cache reads and writes in its prompt tokens.
fn reported(raw: &wire::stream::LlmUsage) -> Usage {
    Usage {
        input: raw.prompt_tokens,
        cached: raw.cached_tokens.or(raw.cache_read_tokens).unwrap_or(0),
        output: raw.completion_tokens,
    }
}

fn usage(raw: &MessageUsage) -> Usage {
    Usage {
        input: raw.prompt_tokens,
        cached: raw.cached_tokens,
        output: raw.completion_tokens,
    }
}

/// A session's transcript turns, in order.
pub fn transcript(raw: Vec<HarnessTranscriptTurn>) -> Vec<Turn> {
    let mut tasks = 0;
    raw.into_iter().map(|raw| turn(raw, &mut tasks)).collect()
}

/// One transcript turn; `tasks` counts the tasks Claude created in the turns before it, which is how
/// Claude numbers them.
fn turn(raw: HarnessTranscriptTurn, tasks: &mut u32) -> Turn {
    let ends_with_text = raw.last_block.is_none_or(|block| block == TranscriptBlock::Text);
    Turn {
        ends_with_text,
        role: match raw.role {
            SessionMessageRole::User => Role::User,
            SessionMessageRole::Assistant => Role::Assistant,
        },
        text: raw.text,
        thinking: raw.thinking.unwrap_or_default(),
        model: raw.model,
        usage: raw.usage.as_ref().map(usage).unwrap_or_default(),
        started_at_ms: raw.started_at_ms,
        ended_at_ms: raw.ended_at_ms,
        files: raw
            .attachments
            .unwrap_or_default()
            .into_iter()
            .map(|file| model::prompt::SentFile {
                media_type: file.media_type,
                name: file.name,
                upload: file.upload_id,
            })
            .collect(),
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
                let given = tool.input.as_ref().or(tool.args.as_ref());
                let input = crate::tool_input::claude_task(&tool.name, given, tasks)
                    .unwrap_or_else(|| crate::tool_input::parse(&tool.name, given));
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
    Some(ContextUsage {
        used,
        window: Some(window),
    })
}

fn driving(
    driver: Option<SessionDriver>,
    busy: bool,
    queued: Vec<String>,
    approval: Option<wire::harness::TerminalApproval>,
) -> Driving {
    Driving {
        driver: driver.map(|driver| match driver {
            SessionDriver::Chat => Driver::Chat,
            SessionDriver::Terminal => Driver::Terminal,
        }),
        busy,
        queued,
        approval: approval.map(|approval| TerminalApproval {
            input: crate::tool_input::parse(&approval.tool, approval.input.as_ref()),
            tool: approval.tool,
        }),
    }
}

/// The names of the tools a session stream has running, so a later update to one's input parses as
/// that tool's.
#[derive(Debug, Default)]
pub struct Running(HashMap<String, String>);

pub fn event(raw: HarnessEvent, running: &mut Running) -> Option<SessionEvent> {
    Some(match raw {
        HarnessEvent::AssistantDelta { text, .. } => SessionEvent::AssistantText(text),
        HarnessEvent::ReasoningDelta { text, .. } => SessionEvent::Reasoning(text),
        HarnessEvent::ToolUse {
            tool_call_id,
            name,
            input,
            ..
        } => {
            running.0.insert(tool_call_id.clone(), name.clone());
            SessionEvent::ToolStarted {
                id: tool_call_id,
                input: crate::tool_input::parse(&name, Some(&input)),
                name,
            }
        }
        HarnessEvent::ToolUpdate {
            tool_call_id, input, ..
        } => {
            let name = running.0.get(&tool_call_id)?;
            SessionEvent::ToolInput {
                input: crate::tool_input::parse(name, Some(&input)),
                id: tool_call_id,
            }
        }
        HarnessEvent::ToolResult {
            tool_call_id,
            name,
            output,
            is_error,
            ..
        } => {
            running.0.remove(&tool_call_id);
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
        HarnessEvent::Plan { entries, .. } => SessionEvent::Plan(plan(entries)),
        HarnessEvent::TurnComplete { usage, model_ms, .. } => {
            running.0.clear();
            SessionEvent::TurnComplete {
                usage: usage.as_ref().map(reported),
                model_ms,
            }
        }
        HarnessEvent::Context { used, window, .. } => SessionEvent::Context(ContextUsage { used, window }),
        HarnessEvent::Error { message, .. } => SessionEvent::Error(message),
        HarnessEvent::Transcript {
            turns,
            context_window,
            ..
        } => {
            let usage = context(&turns, context_window);
            SessionEvent::Snapshot {
                turns: transcript(turns),
                context: usage,
            }
        }
        HarnessEvent::Status(frame) => SessionEvent::Activity(match frame.status {
            HarnessStatus::Working => Activity::Working,
            HarnessStatus::Blocked => Activity::Blocked,
            HarnessStatus::Idle => Activity::Idle,
        }),
        HarnessEvent::Driver {
            driver,
            busy,
            queued,
            approval,
            ..
        } => SessionEvent::Driving(driving(driver, busy, queued, approval)),
        HarnessEvent::SessionUpdated { blocked, .. } => SessionEvent::Waiting(blocked?),
        HarnessEvent::SessionCreated { .. } | HarnessEvent::Prompt { .. } => return None,
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

use super::{Activity, Approval, Role, SessionEvent, SessionView, ToolCall, ToolState, Turn};
use crate::plan::{StepStatus, Todo, Todos};
use crate::prompt::Prompt;
use crate::tools::{ToolInput, ToolOutput};
use crate::usage::Usage;

fn said(role: Role, text: &str) -> Turn {
    Turn {
        role,
        text: text.into(),
        thinking: String::new(),
        tools: Vec::new(),
        ends_with_text: true,
        model: None,
        usage: Usage::default(),
        files: Vec::new(),
        started_at_ms: None,
        ended_at_ms: None,
    }
}

#[test]
fn a_taken_prompt_shows_until_the_transcript_that_records_it_arrives() {
    let mut view = SessionView::default();
    view.accepted(&Prompt::new("hi"), 1);
    let texts = |view: &SessionView| {
        view.turns()
            .iter()
            .map(|turn| turn.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(&view), ["hi"]);
    view.apply(SessionEvent::Snapshot {
        turns: vec![said(Role::User, "hi"), said(Role::Assistant, "hello")],
        context: None,
    });
    assert_eq!(texts(&view), ["hi", "hello"]);
}

#[test]
fn a_steered_prompt_lands_between_the_reply_so_far_and_the_rest() {
    let mut view = SessionView::default();
    view.apply(SessionEvent::AssistantText("Looking".into()));
    view.steered(&Prompt::new("use ripgrep"), 1);
    view.apply(SessionEvent::AssistantText("Using rg".into()));
    let texts: Vec<&str> = view
        .turns()
        .iter()
        .chain(view.live())
        .map(|turn| turn.text.as_str())
        .collect();
    assert_eq!(texts, ["Looking", "use ripgrep", "Using rg"]);
}

#[test]
fn a_todo_tool_keeps_the_plan_and_a_reopened_transcript_rebuilds_it() {
    let todos = |status| {
        ToolInput::Todos(Todos {
            items: vec![Todo {
                id: None,
                content: "test".into(),
                status: Some(status),
            }],
            merge: false,
        })
    };
    let mut view = SessionView::default();
    view.apply(SessionEvent::ToolStarted {
        id: "t1".into(),
        name: "todo_write".into(),
        input: todos(StepStatus::InProgress),
    });
    assert_eq!(
        view.plan().current().map(|step| step.content.as_str()),
        Some("test")
    );
    let mut done = said(Role::Assistant, "");
    done.tools.push(ToolCall {
        id: Some("t2".into()),
        name: "todo_write".into(),
        state: ToolState::Done,
        input: todos(StepStatus::Completed),
        output: None,
    });
    let mut reopened = SessionView::default();
    reopened.apply(SessionEvent::Snapshot {
        turns: vec![done],
        context: None,
    });
    assert_eq!(reopened.plan().done(), 1);
    assert!(reopened.plan().finished());
}

#[test]
fn text_after_tools_starts_a_paragraph_as_the_transcript_joins_it() {
    let mut view = SessionView::default();
    view.apply(SessionEvent::AssistantText("Starting.".into()));
    view.apply(SessionEvent::ToolStarted {
        id: "t1".into(),
        name: "Bash".into(),
        input: ToolInput::Unknown,
    });
    view.apply(SessionEvent::AssistantText("One.".into()));
    assert_eq!(
        view.live().map(|turn| turn.text.as_str()),
        Some("Starting.\n\nOne.")
    );
}

#[test]
fn deltas_build_one_live_turn() {
    let mut view = SessionView::default();
    view.apply(SessionEvent::AssistantText("Hel".into()));
    view.apply(SessionEvent::AssistantText("lo".into()));
    view.apply(SessionEvent::ToolStarted {
        id: "t1".into(),
        name: "Bash".into(),
        input: ToolInput::Unknown,
    });
    view.apply(SessionEvent::ToolFinished {
        id: "t1".into(),
        failed: true,
        output: ToolOutput {
            text: "Exit code 1".into(),
            exit_code: Some(1),
        },
    });
    let live = view.live().cloned().unwrap_or_else(|| said(Role::User, ""));
    assert_eq!(live.text, "Hello");
    assert_eq!(live.tools[0].state, ToolState::Failed);
    assert_eq!(view.activity(), Activity::Working);
}

#[test]
fn a_reconnect_drops_a_turn_and_a_failure_the_old_stream_left_open() {
    let mut view = SessionView::default();
    view.apply(SessionEvent::AssistantText("Wor".into()));
    view.apply(SessionEvent::Error("connection reset".into()));
    view.reconnected();
    view.apply(SessionEvent::Snapshot {
        turns: Vec::new(),
        context: None,
    });
    assert!(!view.busy());
    assert_eq!(view.error(), None);
}

#[test]
fn snapshot_after_turn_complete_never_duplicates_the_turn() {
    let mut view = SessionView::default();
    view.apply(SessionEvent::AssistantText("Done.".into()));
    view.apply(SessionEvent::TurnComplete(None));
    assert_eq!(view.turns().len(), 1);
    let turns = vec![said(Role::User, "go"), said(Role::Assistant, "Done.")];
    view.apply(SessionEvent::Snapshot {
        turns: turns.clone(),
        context: None,
    });
    assert_eq!(view.turns(), turns.as_slice());
    assert!(view.live().is_none());
}

#[test]
fn an_open_approval_means_the_session_needs_you() {
    let mut view = SessionView::default();
    let approval = Approval {
        request_id: "r1".into(),
        tool: "Write".into(),
        reason: None,
    };
    view.apply(SessionEvent::ApprovalRequested(approval.clone()));
    view.apply(SessionEvent::ApprovalRequested(approval));
    assert_eq!(view.approvals().len(), 1);
    assert!(view.needs_you());
    view.apply(SessionEvent::ApprovalResolved {
        request_id: "r1".into(),
    });
    assert!(!view.needs_you());
}

#[test]
fn a_question_stays_open_until_a_reply_follows_it() {
    let question = crate::tools::Question {
        question: Some("Which?".into()),
        header: None,
        multi_select: false,
        options: Vec::new(),
    };
    let mut view = SessionView::default();
    view.apply(SessionEvent::ToolStarted {
        id: "ask".into(),
        name: "AskUserQuestion".into(),
        input: ToolInput::Ask(vec![question]),
    });
    view.apply(SessionEvent::AssistantText("Pick one.".into()));
    view.apply(SessionEvent::TurnComplete(None));
    assert!(view.open_questions().is_some());
    let mut turns = view.turns().to_vec();
    turns.push(said(Role::User, "Which"));
    view.apply(SessionEvent::Snapshot { turns, context: None });
    assert!(view.open_questions().is_none());
}

#[test]
fn a_snapshot_keeps_the_live_output_of_a_tool_it_lists_without_one() {
    let mut view = SessionView::default();
    view.apply(SessionEvent::ToolStarted {
        id: "t1".into(),
        name: "Bash".into(),
        input: ToolInput::Unknown,
    });
    view.apply(SessionEvent::ToolFinished {
        id: "t1".into(),
        failed: false,
        output: ToolOutput {
            text: "hello".into(),
            exit_code: None,
        },
    });
    let mut settled = said(Role::Assistant, "done");
    settled.tools.push(super::ToolCall {
        id: Some("t1".into()),
        name: "Bash".into(),
        state: ToolState::Done,
        input: ToolInput::Unknown,
        output: None,
    });
    view.apply(SessionEvent::Snapshot {
        turns: vec![settled],
        context: None,
    });
    let kept = view.turns()[0].tools[0]
        .output
        .as_ref()
        .map(|output| output.text.as_str());
    assert_eq!(kept, Some("hello"));
}

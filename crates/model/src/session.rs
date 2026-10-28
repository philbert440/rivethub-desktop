use crate::SessionId;
use crate::tools::{Question, ToolInput, ToolOutput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: Option<String>,
    pub name: String,
    pub state: ToolState,
    pub input: ToolInput,
    pub output: Option<ToolOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub role: Role,
    pub text: String,
    pub thinking: String,
    pub tools: Vec<ToolCall>,
    pub ends_with_text: bool,
    pub model: Option<String>,
    pub output_tokens: u64,
}

impl Turn {
    fn assistant() -> Self {
        Self {
            role: Role::Assistant,
            text: String::new(),
            thinking: String::new(),
            tools: Vec::new(),
            ends_with_text: false,
            model: None,
            output_tokens: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUsage {
    pub used: u64,
    pub window: u64,
}

impl ContextUsage {
    pub fn fraction(self) -> f32 {
        if self.window == 0 {
            return 0.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "token counts stay far below f32's exact range"
        )]
        let ratio = self.used as f32 / self.window as f32;
        ratio.clamp(0.0, 1.0)
    }

    pub fn nearly_full(self) -> bool {
        self.fraction() >= 0.8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    #[default]
    Idle,
    Working,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Idle,
    Ended,
    Failed,
}

impl SessionStatus {
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Ended | Self::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: SessionId,
    pub title: Option<String>,
    pub status: SessionStatus,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub updated_at_ms: i64,
    pub parent: Option<SessionId>,
    pub node: Option<String>,
    pub blocked: bool,
}

impl SessionSummary {
    /// Whether a typed filter finds this session by its title, folder, harness or model.
    pub fn matches(&self, query: &str) -> bool {
        crate::search::mentions(
            [
                self.title.as_deref().unwrap_or_default(),
                self.cwd.as_deref().unwrap_or_default(),
                self.id.harness().display_name(),
                self.model.as_deref().unwrap_or_default(),
            ],
            query,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub request_id: String,
    pub tool: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    AllowForSession,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    Snapshot {
        turns: Vec<Turn>,
        context: Option<ContextUsage>,
    },
    AssistantText(String),
    Reasoning(String),
    ToolStarted {
        id: String,
        name: String,
        input: ToolInput,
    },
    ToolFinished {
        id: String,
        failed: bool,
        output: ToolOutput,
    },
    ApprovalRequested(Approval),
    ApprovalResolved {
        request_id: String,
    },
    TurnComplete,
    Activity(Activity),
    Error(String),
}

#[derive(Debug, Clone, Default)]
pub struct SessionView {
    turns: Vec<Turn>,
    live: Option<Turn>,
    approvals: Vec<Approval>,
    context: Option<ContextUsage>,
    activity: Activity,
    error: Option<String>,
}

impl SessionView {
    pub fn apply(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Snapshot { mut turns, context } => {
                self.keep_outputs(&mut turns);
                self.turns = turns;
                self.live = None;
                if context.is_some() {
                    self.context = context;
                }
            }
            SessionEvent::AssistantText(text) => {
                let turn = self.live_turn();
                turn.text.push_str(&text);
                turn.ends_with_text = true;
            }
            SessionEvent::Reasoning(text) => self.live_turn().thinking.push_str(&text),
            SessionEvent::ToolStarted { id, name, input } => {
                self.live_turn().tools.push(ToolCall {
                    id: Some(id),
                    name,
                    state: ToolState::Running,
                    input,
                    output: None,
                });
            }
            SessionEvent::ToolFinished { id, failed, output } => {
                let state = if failed {
                    ToolState::Failed
                } else {
                    ToolState::Done
                };
                if let Some(tool) = self
                    .live
                    .as_mut()
                    .and_then(|turn| turn.tools.iter_mut().find(|tool| tool.id.as_deref() == Some(&id)))
                {
                    tool.state = state;
                    tool.output = Some(output);
                }
            }
            SessionEvent::ApprovalRequested(approval) => {
                self.approvals
                    .retain(|pending| pending.request_id != approval.request_id);
                self.approvals.push(approval);
            }
            SessionEvent::ApprovalResolved { request_id } => {
                self.approvals.retain(|pending| pending.request_id != request_id);
            }
            SessionEvent::TurnComplete => {
                if let Some(turn) = self.live.take() {
                    self.turns.push(turn);
                }
                self.activity = Activity::Idle;
            }
            SessionEvent::Activity(activity) => self.activity = activity,
            SessionEvent::Error(message) => self.error = Some(message),
        }
    }

    fn live_turn(&mut self) -> &mut Turn {
        self.activity = Activity::Working;
        self.live.get_or_insert_with(Turn::assistant)
    }

    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    pub fn live(&self) -> Option<&Turn> {
        self.live.as_ref()
    }

    fn keep_outputs(&self, incoming: &mut [Turn]) {
        let mut known: std::collections::HashMap<&str, &ToolOutput> = self
            .turns
            .iter()
            .chain(self.live.as_ref())
            .flat_map(|turn| &turn.tools)
            .filter_map(|tool| Some((tool.id.as_deref()?, tool.output.as_ref()?)))
            .collect();
        for tool in incoming.iter_mut().flat_map(|turn| &mut turn.tools) {
            if tool.output.is_none()
                && let Some(output) = tool.id.as_deref().and_then(|id| known.remove(id))
            {
                tool.output = Some(output.clone());
            }
        }
    }

    /// The questions the newest turn asked with an ask-user tool, until a reply follows it.
    pub fn open_questions(&self) -> Option<(&ToolCall, &[Question])> {
        let newest = self.live.as_ref().or_else(|| self.turns.last())?;
        if newest.role != Role::Assistant {
            return None;
        }
        newest.tools.iter().rev().find_map(|tool| match &tool.input {
            ToolInput::Ask(questions) if !questions.is_empty() => Some((tool, questions.as_slice())),
            _ => None,
        })
    }

    pub fn approvals(&self) -> &[Approval] {
        &self.approvals
    }

    pub fn context(&self) -> Option<ContextUsage> {
        self.context
    }

    pub fn activity(&self) -> Activity {
        self.activity
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn busy(&self) -> bool {
        self.live.is_some() || self.activity != Activity::Idle
    }

    pub fn needs_you(&self) -> bool {
        !self.approvals.is_empty() || self.activity == Activity::Blocked
    }
}

#[cfg(test)]
mod tests {
    use super::{Activity, Approval, Role, SessionEvent, SessionView, ToolState, Turn};
    use crate::tools::{ToolInput, ToolOutput};

    fn said(role: Role, text: &str) -> Turn {
        Turn {
            role,
            text: text.into(),
            thinking: String::new(),
            tools: Vec::new(),
            ends_with_text: true,
            model: None,
            output_tokens: 0,
        }
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
    fn snapshot_after_turn_complete_never_duplicates_the_turn() {
        let mut view = SessionView::default();
        view.apply(SessionEvent::AssistantText("Done.".into()));
        view.apply(SessionEvent::TurnComplete);
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
        view.apply(SessionEvent::TurnComplete);
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
}

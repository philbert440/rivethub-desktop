use crate::SessionId;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub role: Role,
    pub text: String,
    pub thinking: String,
    pub tools: Vec<ToolCall>,
    pub ends_with_text: bool,
}

impl Turn {
    fn assistant() -> Self {
        Self {
            role: Role::Assistant,
            text: String::new(),
            thinking: String::new(),
            tools: Vec::new(),
            ends_with_text: false,
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
    },
    ToolFinished {
        id: String,
        failed: bool,
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
            SessionEvent::Snapshot { turns, context } => {
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
            SessionEvent::ToolStarted { id, name } => {
                self.live_turn().tools.push(ToolCall {
                    id: Some(id),
                    name,
                    state: ToolState::Running,
                });
            }
            SessionEvent::ToolFinished { id, failed } => {
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

    fn said(role: Role, text: &str) -> Turn {
        Turn {
            role,
            text: text.into(),
            thinking: String::new(),
            tools: Vec::new(),
            ends_with_text: true,
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
        });
        view.apply(SessionEvent::ToolFinished {
            id: "t1".into(),
            failed: true,
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
}

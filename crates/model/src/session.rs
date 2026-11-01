use crate::plan::Plan;
use crate::prompt::{Prompt, SentFile};
use crate::tools::{Question, ToolInput, ToolOutput};
use crate::usage::{Pace, Usage};
use crate::{SessionId, TaskId};

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
    pub usage: Usage,
    pub files: Vec<SentFile>,
    /// When the turn began and ended, in epoch milliseconds, where the harness recorded it.
    pub started_at_ms: Option<i64>,
    pub ended_at_ms: Option<i64>,
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
            usage: Usage::default(),
            files: Vec::new(),
            started_at_ms: None,
            ended_at_ms: None,
        }
    }
}

/// How many tokens the session's context holds, and the size of the model's window when the
/// harness says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUsage {
    pub used: u64,
    pub window: Option<u64>,
}

impl ContextUsage {
    fn window(self) -> Option<u64> {
        self.window.filter(|window| *window > 0)
    }

    pub fn fraction(self) -> Option<f32> {
        let window = self.window()?;
        #[expect(
            clippy::cast_precision_loss,
            reason = "token counts stay far below f32's exact range"
        )]
        let ratio = self.used as f32 / window as f32;
        Some(ratio.clamp(0.0, 1.0))
    }

    /// The share of the window in use, as a whole percent.
    pub fn percent(self) -> Option<u64> {
        let window = self.window()?;
        Some(
            self.used
                .min(window)
                .saturating_mul(100)
                .saturating_add(window / 2)
                / window,
        )
    }

    pub fn nearly_full(self) -> bool {
        self.fraction().is_some_and(|fraction| fraction >= 0.8)
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
    pub effort: Option<String>,
    pub cwd: Option<String>,
    pub updated_at_ms: i64,
    pub parent: Option<SessionId>,
    /// The delegated task the session runs, for one another session started as a task.
    pub task: Option<TaskId>,
    pub node: Option<String>,
    pub blocked: bool,
}

/// What stopping a delegated task found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStop {
    Stopped,
    /// It had already finished, failed or been stopped.
    AlreadyOver,
}

impl SessionSummary {
    /// A session as far as rh remembers it, for one no node lists any more.
    pub fn remembered(
        id: SessionId,
        title: Option<String>,
        updated_at_ms: i64,
        node: Option<String>,
    ) -> Self {
        Self {
            id,
            title,
            status: SessionStatus::Idle,
            model: None,
            effort: None,
            cwd: None,
            updated_at_ms,
            parent: None,
            task: None,
            node,
            blocked: false,
        }
    }

    /// A subagent: a transcript inside its parent's, which goes with its parent and has no
    /// terminal of its own. A delegated task's session is a session in its own right.
    pub fn inside_parent(&self) -> bool {
        self.parent.is_some() && self.task.is_none()
    }

    /// What to call the session where it isn't on screen: its title, or before it has one,
    /// its harness and node, such as "Grok on ct112".
    pub fn name(&self) -> String {
        if let Some(title) = self.title.as_deref().filter(|title| !title.trim().is_empty()) {
            return title.to_owned();
        }
        let harness = self.id.harness().display_name();
        match &self.node {
            Some(node) => format!("{harness} on {node}"),
            None => format!("{harness} session"),
        }
    }

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

/// What a running session lets a turn change, as its node reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Switches {
    pub model: bool,
    pub effort: bool,
}

/// A session as its node describes it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Described {
    pub summary: SessionSummary,
    pub switches: Switches,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub request_id: String,
    pub tool: String,
    pub reason: Option<String>,
}

/// Which of a session's two views holds it: rh lets one run its harness at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    Chat,
    Terminal,
}

/// A permission prompt the session's terminal shows, which only the terminal can answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalApproval {
    pub tool: String,
    pub input: ToolInput,
}

/// Who holds the session as its node reports it, and what waits for them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Driving {
    pub driver: Option<Driver>,
    /// A turn runs, as the node knows: a chat turn, or a prompt submitted in the terminal whose
    /// end it hasn't heard of.
    pub busy: bool,
    /// Chat messages the node holds until the turn running in the other view ends, oldest first.
    pub queued: Vec<String>,
    pub approval: Option<TerminalApproval>,
}

impl Driving {
    pub fn by_terminal(&self) -> bool {
        self.driver == Some(Driver::Terminal)
    }
}

/// The messages that left the front of `before`'s queue to give `after`'s: each one started.
fn started<'a>(before: &'a [String], after: &[String]) -> &'a [String] {
    let kept = (0..=before.len().min(after.len()))
        .rev()
        .find(|kept| after.starts_with(&before[before.len() - kept..]))
        .unwrap_or(0);
    &before[..before.len() - kept]
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
    /// A running tool's input as the harness now names it, whole.
    ToolInput {
        id: String,
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
    /// The agent's to-do list, whole, each time it changes.
    Plan(Plan),
    /// How full the session's context is now, as its harness counts it.
    Context(ContextUsage),
    /// The turn ended, with the tokens it used and the time the model spent on it, where the
    /// node reported them.
    TurnComplete {
        usage: Option<Usage>,
        model_ms: Option<u64>,
    },
    Activity(Activity),
    Driving(Driving),
    /// Whether the session waits for you now: a question or approval it shows, in the terminal
    /// too, until it is answered.
    Waiting(bool),
    Error(String),
}

#[derive(Debug, Clone, Default)]
pub struct SessionView {
    turns: Vec<Turn>,
    live: Option<Turn>,
    approvals: Vec<Approval>,
    context: Option<ContextUsage>,
    /// The harness's own count, which outranks the one read from the transcript until a turn
    /// the harness didn't count runs in the terminal.
    counted: Option<ContextUsage>,
    activity: Activity,
    error: Option<String>,
    /// The stream to the session's node is down, so a turn it showed running can't be followed.
    lost: bool,
    paced: Pace,
    plan: Plan,
    driving: Driving,
    waiting: bool,
}

impl SessionView {
    /// How fast the turns streamed here wrote, all told.
    pub fn pace(&self) -> Pace {
        self.paced
    }

    /// The to-do list the agent last sent while streaming here.
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    pub fn apply(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Snapshot { mut turns, context } => {
                self.keep_outputs(&mut turns);
                if let Some(plan) = written_plan(&turns) {
                    self.plan = plan;
                }
                self.turns = turns;
                self.live = None;
                if context.is_some() {
                    self.context = context;
                }
            }
            SessionEvent::AssistantText(text) => {
                let turn = self.live_turn();
                if !turn.ends_with_text && !turn.tools.is_empty() && !turn.text.is_empty() {
                    turn.text.push_str("\n\n");
                }
                turn.text.push_str(&text);
                turn.ends_with_text = true;
            }
            SessionEvent::Reasoning(text) => self.live_turn().thinking.push_str(&text),
            SessionEvent::ToolStarted { id, name, input } => {
                if let ToolInput::Todos(todos) = &input {
                    self.plan.revise(todos);
                }
                let turn = self.live_turn();
                turn.ends_with_text = false;
                turn.tools.push(ToolCall {
                    id: Some(id),
                    name,
                    state: ToolState::Running,
                    input,
                    output: None,
                });
            }
            SessionEvent::ToolInput { id, input } => {
                let Some(tool) = self
                    .live
                    .as_mut()
                    .and_then(|turn| turn.tools.iter_mut().find(|tool| tool.id.as_deref() == Some(&id)))
                else {
                    return;
                };
                if tool.input == input {
                    return;
                }
                if let ToolInput::Todos(todos) = &input {
                    self.plan.revise(todos);
                }
                tool.input = input;
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
            SessionEvent::TurnComplete { usage, model_ms } => {
                if let Some(turn) = self.live.take() {
                    self.turns.push(turn);
                }
                self.activity = Activity::Idle;
                self.waiting = false;
                if let Some(pace) = usage
                    .zip(model_ms)
                    .and_then(|(usage, ms)| Pace::of(usage.output, ms))
                {
                    self.paced += pace;
                }
            }
            SessionEvent::Context(counted) => self.counted = Some(counted),
            SessionEvent::Plan(plan) => self.plan = plan,
            SessionEvent::Activity(activity) => self.activity = activity,
            SessionEvent::Driving(driving) => {
                if driving.by_terminal() && driving.busy {
                    self.counted = None;
                }
                let before = std::mem::replace(&mut self.driving, driving);
                for text in started(&before.queued, &self.driving.queued) {
                    self.prompted(text.clone(), Vec::new(), None);
                }
            }
            SessionEvent::Waiting(waiting) => self.waiting = waiting,
            SessionEvent::Error(message) => self.error = Some(message),
        }
    }

    /// A fresh stream replays only the transcript, so a turn the old one left running and a
    /// failure it reported no longer stand; a turn still running says so again as it streams.
    pub fn reconnected(&mut self) {
        self.activity = Activity::Idle;
        self.waiting = false;
        self.error = None;
        self.lost = false;
    }

    /// The stream to the session's node went down or couldn't open, as `message` says.
    pub fn disconnected(&mut self, message: String) {
        self.error = Some(message);
        self.lost = true;
    }

    /// The running turn took `prompt` as it went: the reply so far stays above it and goes on
    /// below it.
    pub fn steered(&mut self, prompt: &Prompt, at_ms: i64) {
        if let Some(turn) = self.live.take() {
            self.turns.push(turn);
        }
        self.accepted(prompt, at_ms);
    }

    /// The harness took `prompt` at `at_ms`: it shows as sent until a transcript that records it
    /// arrives, and an earlier failure no longer stands.
    pub fn accepted(&mut self, prompt: &Prompt, at_ms: i64) {
        let files = prompt
            .attachments
            .iter()
            .map(|attachment| SentFile {
                media_type: attachment.media_type.clone(),
                name: Some(attachment.name.clone()),
                upload: None,
            })
            .collect();
        self.prompted(prompt.text.clone(), files, Some(at_ms));
    }

    fn prompted(&mut self, text: String, files: Vec<SentFile>, at_ms: Option<i64>) {
        self.error = None;
        self.activity = Activity::Working;
        self.turns.push(Turn {
            role: Role::User,
            text,
            thinking: String::new(),
            tools: Vec::new(),
            ends_with_text: true,
            model: None,
            usage: Usage::default(),
            files,
            started_at_ms: at_ms,
            ended_at_ms: at_ms,
        });
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

    /// The questions the newest turn asked with an ask-user tool, until a reply follows it or the
    /// tool comes back with the answers, as it does when they were given in the terminal.
    pub fn open_questions(&self) -> Option<(&ToolCall, &[Question])> {
        let newest = self.live.as_ref().or_else(|| self.turns.last())?;
        if newest.role != Role::Assistant {
            return None;
        }
        let (tool, questions) = newest.tools.iter().rev().find_map(|tool| match &tool.input {
            ToolInput::Ask(questions) if !questions.is_empty() => Some((tool, questions.as_slice())),
            _ => None,
        })?;
        (tool.state != ToolState::Done).then_some((tool, questions))
    }

    pub fn approvals(&self) -> &[Approval] {
        &self.approvals
    }

    pub fn driving(&self) -> &Driving {
        &self.driving
    }

    pub fn context(&self) -> Option<ContextUsage> {
        self.counted.or(self.context)
    }

    pub fn activity(&self) -> Activity {
        self.activity
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Whether the stream to the session's node is down until it reconnects.
    pub fn lost(&self) -> bool {
        self.lost
    }

    pub fn busy(&self) -> bool {
        self.live.is_some() || self.activity != Activity::Idle
    }

    /// Whether a turn is still running on an open question: streaming now, or, on a stream opened
    /// mid-turn, an ask tool the transcript still shows running.
    pub fn held_by_question(&self) -> bool {
        self.busy()
            || self
                .open_questions()
                .is_some_and(|(tool, _)| tool.state == ToolState::Running)
    }

    pub fn needs_you(&self) -> bool {
        self.waiting || !self.approvals.is_empty() || self.activity == Activity::Blocked
    }
}

/// The plan the todo tool calls in `turns` add up to, when the agent made any.
fn written_plan(turns: &[Turn]) -> Option<Plan> {
    turns
        .iter()
        .flat_map(|turn| &turn.tools)
        .fold(None, |plan: Option<Plan>, tool| match &tool.input {
            ToolInput::Todos(todos) => {
                let mut plan = plan.unwrap_or_default();
                plan.revise(todos);
                Some(plan)
            }
            _ => plan,
        })
}

#[cfg(test)]
mod tests;

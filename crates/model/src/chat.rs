use std::collections::HashSet;

use crate::prompt::SentFile;
use crate::session::{Role, ToolCall, ToolState, Turn};
use crate::tools::ToolInput;
use crate::usage::Usage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Read,
    Search,
    Edit,
    Command,
    Web,
    Subagent,
    Tool,
}

impl Category {
    pub fn of(tool: &str) -> Self {
        match tool {
            "Read" | "NotebookRead" | "read_file" | "read" => Self::Read,
            "Grep" | "Glob" | "LS" | "grep" | "glob" | "search" => Self::Search,
            "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "search_replace" | "write_file" => Self::Edit,
            "Bash" | "BashOutput" | "KillShell" | "bash" | "shell" | "exec" | "run_terminal_command" => {
                Self::Command
            }
            "WebFetch" | "WebSearch" | "web_fetch" | "web_search" => Self::Web,
            "Agent" | "Task" => Self::Subagent,
            _ => Self::Tool,
        }
    }

    fn done(self, count: usize) -> String {
        let (verb, one, many) = match self {
            Self::Read => ("read", "file", "files"),
            Self::Search => ("ran", "search", "searches"),
            Self::Edit => ("edited", "file", "files"),
            Self::Command => ("ran", "command", "commands"),
            Self::Web => ("made", "web request", "web requests"),
            Self::Subagent => ("ran", "subagent", "subagents"),
            Self::Tool => ("used", "tool", "tools"),
        };
        format!("{verb} {count} {}", if count == 1 { one } else { many })
    }

    fn running(self) -> &'static str {
        match self {
            Self::Read => "Reading files",
            Self::Search => "Searching the code",
            Self::Edit => "Editing files",
            Self::Command => "Running commands",
            Self::Web => "Searching the web",
            Self::Subagent => "Coordinating subagents",
            Self::Tool => "Using tools",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Thought(String),
    Note(String),
    Tools(Vec<ToolCall>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeState {
    Working,
    Completed,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    pub prompt: Option<String>,
    pub files: Vec<SentFile>,
    pub steps: Vec<Step>,
    pub answer: String,
    pub state: ExchangeState,
    pub model: Option<String>,
    pub usage: Usage,
    /// When the person sent the prompt, in epoch milliseconds.
    pub asked_at_ms: Option<i64>,
    /// When the exchange began: the prompt, or for a reply that came without one its first turn.
    pub started_at_ms: Option<i64>,
    /// When the last reply turn ended.
    pub ended_at_ms: Option<i64>,
}

impl Exchange {
    fn new(prompt: Option<String>, files: Vec<SentFile>, asked_at_ms: Option<i64>) -> Self {
        Self {
            prompt,
            files,
            steps: Vec::new(),
            answer: String::new(),
            state: ExchangeState::Completed,
            model: None,
            usage: Usage::default(),
            asked_at_ms,
            started_at_ms: asked_at_ms,
            ended_at_ms: None,
        }
    }

    /// How long a finished exchange took, from its start to the end of its last reply turn.
    /// A running exchange, or one whose harness recorded no times, has none.
    pub fn took_ms(&self) -> Option<i64> {
        if self.state == ExchangeState::Working {
            return None;
        }
        let took = self.ended_at_ms? - self.started_at_ms?;
        (took >= 0).then_some(took)
    }

    /// The fold header's verdict: "Completed in 42s", "Stopped after 8s", "Failed after 3m 5s", or
    /// without a time when the exchange has none.
    pub fn verdict(&self) -> String {
        let (word, joint) = match self.state {
            ExchangeState::Failed => ("Failed", "after"),
            ExchangeState::Stopped => ("Stopped", "after"),
            ExchangeState::Working | ExchangeState::Completed => ("Completed", "in"),
        };
        match self.took_ms() {
            Some(took) => format!("{word} {joint} {}", elapsed(took)),
            None => word.to_owned(),
        }
    }

    fn take_answer_as_note(&mut self) {
        if !self.answer.trim().is_empty() {
            self.steps.push(Step::Note(std::mem::take(&mut self.answer)));
        }
        self.answer.clear();
    }

    fn absorb(&mut self, turn: &Turn) {
        self.take_answer_as_note();
        self.started_at_ms = self.started_at_ms.or(turn.started_at_ms);
        self.ended_at_ms = self.ended_at_ms.max(turn.ended_at_ms.or(turn.started_at_ms));
        if turn.model.is_some() {
            self.model.clone_from(&turn.model);
        }
        self.usage += turn.usage;
        if !turn.thinking.trim().is_empty() {
            self.steps.push(Step::Thought(turn.thinking.clone()));
        }
        if turn.tools.is_empty() {
            self.answer.clone_from(&turn.text);
            return;
        }
        let narrated = !turn.text.trim().is_empty() && !turn.ends_with_text;
        if narrated {
            self.steps.push(Step::Note(turn.text.clone()));
        }
        match self.steps.last_mut() {
            Some(Step::Tools(tools)) => tools.extend(turn.tools.iter().cloned()),
            _ => self.steps.push(Step::Tools(turn.tools.clone())),
        }
        if turn.ends_with_text {
            self.answer.clone_from(&turn.text);
        }
    }

    pub fn tool_count(&self) -> usize {
        self.steps
            .iter()
            .map(|step| {
                if let Step::Tools(tools) = step {
                    tools.len()
                } else {
                    0
                }
            })
            .sum()
    }

    pub fn has_process(&self) -> bool {
        !self.steps.is_empty()
    }
}

/// The transcript folded into exchanges. A prompt with no reply yet counts as running while the
/// session is `busy`; once it isn't, it got none.
pub fn exchanges(turns: &[Turn], live: Option<&Turn>, busy: bool) -> Vec<Exchange> {
    let mut out: Vec<Exchange> = Vec::new();
    for turn in turns.iter().chain(live) {
        match turn.role {
            Role::User if is_interruption(&turn.text) => {
                if let Some(exchange) = out.last_mut() {
                    exchange.state = ExchangeState::Stopped;
                }
            }
            Role::User => out.push(Exchange::new(
                Some(turn.text.clone()),
                turn.files.clone(),
                turn.started_at_ms,
            )),
            Role::Assistant => {
                if out.is_empty() {
                    out.push(Exchange::new(None, Vec::new(), None));
                }
                if let Some(exchange) = out.last_mut() {
                    exchange.absorb(turn);
                }
            }
        }
    }
    if let Some(last) = out.last_mut().filter(|last| last.state != ExchangeState::Stopped) {
        let running = live.is_some()
            || last.steps.iter().any(|step| {
                matches!(step, Step::Tools(tools) if tools.iter().any(|tool| tool.state == ToolState::Running))
            });
        last.state = if running {
            ExchangeState::Working
        } else {
            last.state
        };
        if busy
            && last.state == ExchangeState::Completed
            && last.answer.is_empty()
            && last.prompt.is_some()
            && last.steps.is_empty()
        {
            last.state = ExchangeState::Working;
        }
    }
    for exchange in &mut out {
        let failed = exchange.answer.is_empty()
            && exchange.state == ExchangeState::Completed
            && matches!(exchange.steps.last(), Some(Step::Tools(tools)) if tools.last().is_some_and(|tool| tool.state == ToolState::Failed));
        if failed {
            exchange.state = ExchangeState::Failed;
        }
    }
    out
}

/// Everything the exchanges read and wrote.
pub fn totals<'a>(exchanges: impl IntoIterator<Item = &'a Exchange>) -> Usage {
    let mut usage = Usage::default();
    for exchange in exchanges {
        usage += exchange.usage;
    }
    usage
}

/// Whether a transcript that had `count` exchanges, the last in state `last`, moved on to where a
/// reader wants to be in `now`: an exchange added at the end (your send, or a reply that came without
/// one), or the running one finishing.
pub fn arrived(count: usize, last: Option<ExchangeState>, now: &[Exchange]) -> bool {
    now.len() > count
        || (last == Some(ExchangeState::Working)
            && now
                .last()
                .is_some_and(|exchange| exchange.state != ExchangeState::Working))
}

/// A span of time as a reader says it: "42s", "3m 5s", "1h 2m". Anything under a second is "1s".
pub fn elapsed(ms: i64) -> String {
    let seconds = ((ms + 500) / 1000).max(1);
    match seconds {
        ..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

fn is_interruption(text: &str) -> bool {
    text.trim_start().starts_with("[Request interrupted by user")
}

/// Names what a run of tool calls did, e.g. "Read 3 files and ran 2 commands".
/// A file counts once however often the run read or edited it.
pub fn group_summary(tools: &[ToolCall]) -> String {
    let mut tally: Vec<(Category, usize)> = Vec::new();
    let mut files: HashSet<(Category, &str)> = HashSet::new();
    for tool in tools {
        let category = Category::of(&tool.name);
        if let Some(path) = file(&tool.input)
            && !files.insert((category, path))
        {
            continue;
        }
        match tally.iter_mut().find(|(seen, _)| *seen == category) {
            Some((_, count)) => *count += 1,
            None => tally.push((category, 1)),
        }
    }
    let words: Vec<String> = tally
        .iter()
        .take(3)
        .map(|(category, count)| category.done(*count))
        .collect();
    let mut summary = match words.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        [first, second, third, ..] => format!("{first}, {second} and {third}"),
    };
    if tally.len() > 3 {
        summary.push_str(", etc.");
    }
    capitalize(&summary)
}

fn file(input: &ToolInput) -> Option<&str> {
    match input {
        ToolInput::Read { path } | ToolInput::Edit { path, .. } | ToolInput::Write { path, .. } => Some(path),
        _ => None,
    }
}

pub fn running_label(tools: &[ToolCall]) -> Option<&'static str> {
    tools
        .iter()
        .rev()
        .find(|tool| tool.state == ToolState::Running)
        .map(|tool| Category::of(&tool.name).running())
}

pub fn thought_summary(thinking: &str, streaming: bool) -> String {
    let paragraphs: Vec<&str> = thinking
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let finished = if streaming && !thinking.ends_with("\n\n") {
        paragraphs.len().saturating_sub(1)
    } else {
        paragraphs.len()
    };
    let chosen = if streaming {
        finished.checked_sub(1).and_then(|i| paragraphs.get(i))
    } else {
        paragraphs.first()
    };
    chosen
        .and_then(|paragraph| paragraph.lines().next())
        .map(|line| line.replace("**", "").trim().to_owned())
        .unwrap_or_default()
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{ExchangeState, Step, arrived, elapsed, exchanges, group_summary, thought_summary};
    use crate::session::{Role, ToolCall, ToolState, Turn};
    use crate::tools::ToolInput;
    use crate::usage::Usage;

    fn turn(role: Role, text: &str, tools: &[(&str, ToolState)]) -> Turn {
        Turn {
            ends_with_text: tools.is_empty(),
            role,
            text: text.into(),
            thinking: String::new(),
            model: None,
            usage: Usage::default(),
            files: Vec::new(),
            started_at_ms: None,
            ended_at_ms: None,
            tools: tools
                .iter()
                .map(|(name, state)| ToolCall {
                    id: None,
                    name: (*name).into(),
                    state: *state,
                    input: ToolInput::Unknown,
                    output: None,
                })
                .collect(),
        }
    }

    #[test]
    fn a_turn_folds_its_process_and_keeps_the_last_reply_as_the_answer() {
        let turns = [
            turn(Role::User, "fix it", &[]),
            turn(Role::Assistant, "Looking.", &[("Read", ToolState::Done)]),
            turn(
                Role::Assistant,
                "",
                &[("Bash", ToolState::Done), ("Grep", ToolState::Done)],
            ),
            turn(Role::Assistant, "Fixed.", &[]),
            turn(Role::User, "thanks", &[]),
        ];
        let folded = exchanges(&turns, None, true);
        assert_eq!(folded.len(), 2);
        assert_eq!(folded[0].answer, "Fixed.");
        assert_eq!(folded[0].tool_count(), 3);
        assert!(matches!(&folded[0].steps[0], Step::Note(note) if note == "Looking."));
        assert_eq!(folded[0].state, ExchangeState::Completed);
        assert_eq!(folded[1].state, ExchangeState::Working);
    }

    #[test]
    fn a_reply_that_ends_in_text_after_tools_is_the_answer() {
        let mut last = turn(Role::Assistant, "All done.", &[("Bash", ToolState::Done)]);
        last.ends_with_text = true;
        let folded = exchanges(&[turn(Role::User, "go", &[]), last], None, false);
        assert_eq!(folded[0].answer, "All done.");
        assert_eq!(folded[0].tool_count(), 1);
    }

    #[test]
    fn an_interruption_marker_stops_the_exchange_instead_of_starting_one() {
        let turns = [
            turn(Role::User, "go", &[]),
            turn(Role::Assistant, "", &[("Bash", ToolState::Done)]),
            turn(Role::User, "[Request interrupted by user for tool use]", &[]),
        ];
        let folded = exchanges(&turns, None, false);
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].state, ExchangeState::Stopped);
    }

    #[test]
    fn a_live_turn_keeps_the_exchange_working() {
        let turns = [turn(Role::User, "go", &[])];
        let live = turn(Role::Assistant, "Partial", &[]);
        let folded = exchanges(&turns, Some(&live), true);
        assert_eq!(folded[0].state, ExchangeState::Working);
        assert_eq!(folded[0].answer, "Partial");
    }

    #[test]
    fn a_prompt_with_no_reply_is_running_only_while_the_session_is_busy() {
        let asked = [turn(Role::User, "go", &[])];
        let state = |busy| {
            exchanges(&asked, None, busy)
                .first()
                .map(|exchange| exchange.state)
        };
        assert_eq!(state(true), Some(ExchangeState::Working));
        assert_eq!(state(false), Some(ExchangeState::Completed));
    }

    #[test]
    fn a_send_or_a_finished_turn_is_an_arrival_but_streaming_is_not() {
        let asked = exchanges(&[turn(Role::User, "go", &[])], None, true);
        assert!(arrived(0, None, &asked));
        let streaming = exchanges(
            &[turn(Role::User, "go", &[])],
            Some(&turn(Role::Assistant, "Wor", &[])),
            true,
        );
        assert!(!arrived(1, Some(ExchangeState::Working), &streaming));
        let done = exchanges(
            &[turn(Role::User, "go", &[]), turn(Role::Assistant, "Done.", &[])],
            None,
            false,
        );
        assert!(arrived(1, Some(ExchangeState::Working), &done));
        assert!(!arrived(1, Some(ExchangeState::Completed), &done));
    }

    #[test]
    fn group_summaries_count_up_to_three_categories_and_each_file_once() {
        let call = |name: &str, input: ToolInput| ToolCall {
            id: None,
            name: name.into(),
            state: ToolState::Done,
            input,
            output: None,
        };
        let read = |path: &str| call("Read", ToolInput::Read { path: path.into() });
        let tools = [
            read("a.rs"),
            call("Bash", ToolInput::Unknown),
            read("a.rs"),
            read("b.rs"),
            call("Bash", ToolInput::Unknown),
            call("Grep", ToolInput::Unknown),
            call("WebFetch", ToolInput::Unknown),
        ];
        assert_eq!(group_summary(&tools[..2]), "Read 1 file and ran 1 command");
        assert_eq!(
            group_summary(&tools),
            "Read 2 files, ran 2 commands and ran 1 search, etc."
        );
    }

    #[test]
    fn an_exchange_runs_from_its_prompt_to_the_end_of_its_last_reply() {
        let timed = |role, text, started: i64, ended: i64| {
            let mut timed = turn(role, text, &[]);
            timed.started_at_ms = Some(started);
            timed.ended_at_ms = Some(ended);
            timed
        };
        let turns = [
            timed(Role::User, "go", 1_000, 1_000),
            timed(Role::Assistant, "Looking.", 3_000, 20_000),
            timed(Role::Assistant, "Done.", 21_000, 43_400),
        ];
        let done = exchanges(&turns, None, false);
        assert_eq!(done[0].took_ms(), Some(42_400));
        assert_eq!(done[0].verdict(), "Completed in 42s");
        let running = exchanges(&turns[..1], Some(&turns[1]), true);
        assert_eq!(running[0].took_ms(), None);
        let untimed = exchanges(
            &[turn(Role::User, "go", &[]), turn(Role::Assistant, "ok", &[])],
            None,
            false,
        );
        assert_eq!(untimed[0].verdict(), "Completed");
    }

    #[test]
    fn elapsed_time_reads_in_the_largest_two_units() {
        assert_eq!(elapsed(120), "1s");
        assert_eq!(elapsed(185_000), "3m 5s");
        assert_eq!(elapsed(3_720_000), "1h 2m");
    }

    #[test]
    fn a_streaming_thought_only_shows_finished_paragraphs() {
        let thinking = "**Plan** the fix\nmore\n\nNow checking the tes";
        assert_eq!(thought_summary(thinking, true), "Plan the fix");
        assert_eq!(thought_summary(thinking, false), "Plan the fix");
        assert_eq!(thought_summary("only a partial line", true), "");
    }
}

use crate::plan::{StepStatus, Todos};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ToolInput {
    Command {
        command: String,
        description: Option<String>,
    },
    Edit {
        path: String,
        before: String,
        after: String,
    },
    Write {
        path: String,
        content: String,
    },
    Read {
        path: String,
    },
    Search {
        pattern: String,
        scope: Option<String>,
    },
    Fetch {
        target: String,
    },
    Delegate {
        description: String,
    },
    Ask(Vec<Question>),
    Todos(Todos),
    Fields(Vec<(String, String)>),
    #[default]
    Unknown,
}

/// A question an agent puts to you with an ask-user tool such as `AskUserQuestion`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub question: Option<String>,
    pub header: Option<String>,
    pub multi_select: bool,
    pub options: Vec<QuestionOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

impl QuestionOption {
    /// Whether the agent suggests this option, as it marks one by ending its label "(Recommended)".
    pub fn recommended(&self) -> bool {
        self.label.trim_end().ends_with("(Recommended)")
    }
}

/// The next turn that answers `questions` with the labels picked for each, in order. A lone question
/// is answered with its labels; several are each prefixed with their header so the agent can match
/// them up, and unanswered ones are left out.
pub fn answer(questions: &[Question], picked: &[Vec<String>]) -> String {
    if let [_] = questions {
        return picked.first().map(|labels| labels.join(", ")).unwrap_or_default();
    }
    questions
        .iter()
        .zip(picked)
        .filter(|(_, labels)| !labels.is_empty())
        .map(|(question, labels)| {
            let labels = labels.join(", ");
            match question.header.as_deref().or(question.question.as_deref()) {
                Some(prefix) if prefix.ends_with(['?', ':']) => format!("{prefix} {labels}"),
                Some(prefix) => format!("{prefix}: {labels}"),
                None => labels,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub text: String,
    pub exit_code: Option<i32>,
}

pub fn display_path(path: &str, cwd: Option<&str>, home: Option<&str>) -> String {
    let inside = |root: &str| {
        let root = root.trim_end_matches('/');
        path.strip_prefix(root)
            .and_then(|rest| rest.strip_prefix('/'))
            .filter(|rest| !rest.is_empty())
    };
    if let Some(relative) = cwd.and_then(inside) {
        return relative.to_owned();
    }
    if let Some(relative) = home.and_then(inside) {
        return format!("~/{relative}");
    }
    path.to_owned()
}

pub fn label(name: &str, input: &ToolInput, cwd: Option<&str>, home: Option<&str>) -> String {
    let first_line = |text: &str| text.lines().next().unwrap_or_default().trim().to_owned();
    match input {
        ToolInput::Command { command, description } => description
            .as_deref()
            .map(str::trim)
            .filter(|description| !description.is_empty())
            .map_or_else(|| first_line(command), str::to_owned),
        ToolInput::Edit { path, .. } | ToolInput::Write { path, .. } | ToolInput::Read { path } => {
            display_path(path, cwd, home)
        }
        ToolInput::Search { pattern, scope } => match scope {
            Some(scope) => format!("{pattern} in {}", display_path(scope, cwd, home)),
            None => pattern.clone(),
        },
        ToolInput::Fetch { target } => target.clone(),
        ToolInput::Delegate { description } => first_line(description),
        ToolInput::Ask(questions) => questions
            .iter()
            .find_map(|question| question.question.as_deref().or(question.header.as_deref()))
            .map_or_else(|| name.to_owned(), first_line),
        ToolInput::Todos(todos) => {
            let named = || todos.items.iter().filter(|todo| !todo.content.is_empty());
            named()
                .find(|todo| todo.status == Some(StepStatus::InProgress))
                .or_else(|| named().next())
                .map_or_else(
                    || match todos.items.len() {
                        0 => name.to_owned(),
                        1 => "Updates 1 item".to_owned(),
                        count => format!("Updates {count} items"),
                    },
                    |todo| first_line(&todo.content),
                )
        }
        ToolInput::Fields(fields) => fields
            .first()
            .map_or_else(|| name.to_owned(), |(_, value)| first_line(value)),
        ToolInput::Unknown => name.to_owned(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Same,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Line { change: Change, text: String },
    Gap(usize),
}

const CONTEXT: usize = 3;
const TABLE_LIMIT: usize = 250_000;

pub fn diff(before: &str, after: &str) -> Vec<DiffLine> {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let head = old
        .iter()
        .zip(&new)
        .take_while(|(left, right)| left == right)
        .count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let mut changes: Vec<(Change, &str)> = old[..head].iter().map(|line| (Change::Same, *line)).collect();
    changes.extend(middle(&old[head..old.len() - tail], &new[head..new.len() - tail]));
    changes.extend(old[old.len() - tail..].iter().map(|line| (Change::Same, *line)));
    fold_context(&changes)
}

fn middle<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Change, &'a str)> {
    if old.len().saturating_mul(new.len()) > TABLE_LIMIT {
        return old
            .iter()
            .map(|line| (Change::Removed, *line))
            .chain(new.iter().map(|line| (Change::Added, *line)))
            .collect();
    }
    let width = new.len() + 1;
    let mut table = vec![0_u32; (old.len() + 1) * width];
    for row in (0..old.len()).rev() {
        for col in (0..new.len()).rev() {
            table[row * width + col] = if old[row] == new[col] {
                table[(row + 1) * width + col + 1] + 1
            } else {
                table[(row + 1) * width + col].max(table[row * width + col + 1])
            };
        }
    }
    let (mut row, mut col) = (0, 0);
    let mut out = Vec::with_capacity(old.len() + new.len());
    while row < old.len() && col < new.len() {
        if old[row] == new[col] {
            out.push((Change::Same, old[row]));
            row += 1;
            col += 1;
        } else if table[(row + 1) * width + col] >= table[row * width + col + 1] {
            out.push((Change::Removed, old[row]));
            row += 1;
        } else {
            out.push((Change::Added, new[col]));
            col += 1;
        }
    }
    out.extend(old[row..].iter().map(|line| (Change::Removed, *line)));
    out.extend(new[col..].iter().map(|line| (Change::Added, *line)));
    out
}

fn fold_context(changes: &[(Change, &str)]) -> Vec<DiffLine> {
    let near_change = |index: usize| {
        let low = index.saturating_sub(CONTEXT);
        let high = (index + CONTEXT + 1).min(changes.len());
        changes[low..high]
            .iter()
            .any(|(change, _)| *change != Change::Same)
    };
    let mut out = Vec::new();
    let mut hidden = 0;
    for (index, (change, text)) in changes.iter().enumerate() {
        if *change == Change::Same && !near_change(index) {
            hidden += 1;
            continue;
        }
        if hidden > 0 {
            out.push(DiffLine::Gap(hidden));
            hidden = 0;
        }
        out.push(DiffLine::Line {
            change: *change,
            text: (*text).to_owned(),
        });
    }
    if hidden > 0 {
        out.push(DiffLine::Gap(hidden));
    }
    out
}

pub fn counts(lines: &[DiffLine]) -> (usize, usize) {
    lines.iter().fold((0, 0), |(added, removed), line| match line {
        DiffLine::Line {
            change: Change::Added,
            ..
        } => (added + 1, removed),
        DiffLine::Line {
            change: Change::Removed,
            ..
        } => (added, removed + 1),
        _ => (added, removed),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(header: &str) -> Question {
        Question {
            question: Some(format!("Which {header}?")),
            header: Some(header.into()),
            multi_select: true,
            options: Vec::new(),
        }
    }

    #[test]
    fn an_answer_names_each_question_only_when_there_are_several() {
        let picks = |labels: &[&str]| labels.iter().map(|label| (*label).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            answer(&[asked("Auth")], &[picks(&["JWT", "OAuth"])]),
            "JWT, OAuth"
        );
        assert_eq!(
            answer(
                &[asked("Auth"), asked("Store"), asked("Cache")],
                &[picks(&["JWT"]), picks(&[]), picks(&["Redis"])]
            ),
            "Auth: JWT\nCache: Redis"
        );
        let asked_only = |question: &str| Question {
            question: Some(question.into()),
            header: None,
            multi_select: false,
            options: Vec::new(),
        };
        assert_eq!(
            answer(
                &[asked_only("Which color?"), asked_only("Size")],
                &[picks(&["Green"]), picks(&["Large"])]
            ),
            "Which color? Green\nSize: Large"
        );
    }

    #[test]
    fn an_option_is_recommended_only_by_its_closing_mark() {
        let option = |label: &str| QuestionOption {
            label: label.to_owned(),
            description: None,
        };
        assert!(option("Postgres (Recommended) ").recommended());
        assert!(!option("(Recommended) is not Postgres").recommended());
    }

    fn line(change: Change, text: &str) -> DiffLine {
        DiffLine::Line {
            change,
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_changed_line_diffs_as_removed_then_added_with_folded_context() {
        let before = "1\n2\n3\n4\n5\n6\n7\n8\nold\n9";
        let after = "1\n2\n3\n4\n5\n6\n7\n8\nnew\n9";
        assert_eq!(
            diff(before, after),
            [
                DiffLine::Gap(5),
                line(Change::Same, "6"),
                line(Change::Same, "7"),
                line(Change::Same, "8"),
                line(Change::Removed, "old"),
                line(Change::Added, "new"),
                line(Change::Same, "9"),
            ]
        );
        assert_eq!(counts(&diff(before, after)), (1, 1));
    }

    #[test]
    fn insertions_keep_the_untouched_lines_in_place() {
        assert_eq!(
            diff("a\nc", "a\nb\nc"),
            [
                line(Change::Same, "a"),
                line(Change::Added, "b"),
                line(Change::Same, "c")
            ]
        );
    }

    #[test]
    fn paths_read_relative_to_the_session_then_home() {
        let cwd = Some("/home/phil/work/app");
        let home = Some("/home/phil");
        assert_eq!(
            display_path("/home/phil/work/app/src/main.rs", cwd, home),
            "src/main.rs"
        );
        assert_eq!(
            display_path("/home/phil/.config/x.toml", cwd, home),
            "~/.config/x.toml"
        );
        assert_eq!(display_path("/etc/hosts", cwd, home), "/etc/hosts");
    }

    #[test]
    fn a_command_row_prefers_its_description() {
        let described = ToolInput::Command {
            command: "cargo build --release".into(),
            description: Some("Build the release binary".into()),
        };
        let bare = ToolInput::Command {
            command: "ls -la\npwd".into(),
            description: None,
        };
        assert_eq!(label("Bash", &described, None, None), "Build the release binary");
        assert_eq!(label("Bash", &bare, None, None), "ls -la");
    }
}

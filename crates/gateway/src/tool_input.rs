use model::chat::Category;
use model::tools::{Question, QuestionOption, ToolInput, ToolOutput};
use serde_json::Value;
use wire::JsonObject;

const OUTPUT_LIMIT: usize = 64 * 1024;
const FIELD_LIMIT: usize = 240;
const FIELDS_SHOWN: usize = 6;

fn text(input: &JsonObject, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match input.get(key)? {
        Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
        Value::Array(parts) => {
            let joined: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
            (!joined.is_empty()).then(|| joined.join(" "))
        }
        _ => None,
    })
}

fn edits(input: &JsonObject) -> Option<(String, String)> {
    let Value::Array(edits) = input.get("edits")? else {
        return None;
    };
    let side = |key: &str| {
        edits
            .iter()
            .filter_map(|edit| edit.get(key).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some((side("old_string"), side("new_string")))
}

/// Claude's `AskUserQuestion` nests up to four questions; Grok's `ask_user_question` and `RivetOS`'s
/// `ask_user` ask one, with options or choices that are labels or `{label}` objects.
fn questions(input: &JsonObject) -> Vec<Question> {
    let option = |value: &Value| {
        let label = match value {
            Value::String(label) => label.trim(),
            other => other.get("label").and_then(Value::as_str)?.trim(),
        };
        (!label.is_empty()).then(|| QuestionOption {
            label: label.to_owned(),
            description: value
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned),
        })
    };
    let question = |value: &Value| {
        let text = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        let options: Vec<QuestionOption> = ["options", "choices"]
            .iter()
            .filter_map(|key| value.get(key).and_then(Value::as_array))
            .flatten()
            .filter_map(option)
            .collect();
        (!options.is_empty()).then(|| Question {
            question: text("question"),
            header: text("header"),
            multi_select: value.get("multiSelect").and_then(Value::as_bool).unwrap_or(false),
            options,
        })
    };
    match input.get("questions").and_then(Value::as_array) {
        Some(nested) => nested.iter().filter_map(question).collect(),
        None => question(&Value::Object(input.as_map().clone()))
            .into_iter()
            .collect(),
    }
}

fn fields(input: &JsonObject) -> ToolInput {
    let shown: Vec<(String, String)> = input
        .as_map()
        .iter()
        .filter_map(|(key, value)| {
            let rendered = match value {
                Value::String(text) => text.clone(),
                Value::Null => return None,
                other => other.to_string(),
            };
            Some((key.clone(), rendered.chars().take(FIELD_LIMIT).collect()))
        })
        .take(FIELDS_SHOWN)
        .collect();
    if shown.is_empty() {
        ToolInput::Unknown
    } else {
        ToolInput::Fields(shown)
    }
}

pub fn parse(name: &str, input: Option<&JsonObject>) -> ToolInput {
    let Some(input) = input else {
        return ToolInput::Unknown;
    };
    if matches!(name, "AskUserQuestion" | "ask_user_question" | "ask_user") {
        let asked = questions(input);
        if !asked.is_empty() {
            return ToolInput::Ask(asked);
        }
    }
    let path = || text(input, &["file_path", "path", "notebook_path", "filename"]);
    let parsed = match Category::of(name) {
        Category::Command => text(input, &["command", "cmd"]).map(|command| ToolInput::Command {
            command,
            description: text(input, &["description"]),
        }),
        Category::Edit => path().map(|path| {
            if let Some(content) = text(input, &["content", "file_text"]) {
                return ToolInput::Write { path, content };
            }
            let (before, after) = edits(input).unwrap_or_else(|| {
                (
                    text(input, &["old_string", "old_str"]).unwrap_or_default(),
                    text(input, &["new_string", "new_str"]).unwrap_or_default(),
                )
            });
            ToolInput::Edit { path, before, after }
        }),
        Category::Read => path().map(|path| ToolInput::Read { path }),
        Category::Search => text(input, &["pattern", "query", "glob"]).map(|pattern| ToolInput::Search {
            pattern,
            scope: text(input, &["path", "include"]),
        }),
        Category::Web => text(input, &["url", "query"]).map(|target| ToolInput::Fetch { target }),
        Category::Subagent => {
            text(input, &["description", "prompt"]).map(|description| ToolInput::Delegate { description })
        }
        Category::Tool => None,
    };
    parsed.unwrap_or_else(|| fields(input))
}

fn tail(text: &str) -> String {
    if text.len() <= OUTPUT_LIMIT {
        return text.to_owned();
    }
    let mut start = text.len() - OUTPUT_LIMIT;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

pub fn output(name: &str, text: &str, failed: bool) -> ToolOutput {
    let exit_code = (failed && Category::of(name) == Category::Command)
        .then(|| {
            text.strip_prefix("Exit code ")
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|code| code.parse().ok())
        })
        .flatten();
    ToolOutput {
        text: tail(text),
        exit_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(json: &str) -> JsonObject {
        serde_json::from_str(json).unwrap_or_default()
    }

    #[test]
    fn claude_tool_inputs_become_typed_views() {
        let bash = object(r#"{"command":"cargo test","description":"Run the tests"}"#);
        assert_eq!(
            parse("Bash", Some(&bash)),
            ToolInput::Command {
                command: "cargo test".into(),
                description: Some("Run the tests".into())
            }
        );
        let edit = object(r#"{"file_path":"/a/b.rs","old_string":"x","new_string":"y"}"#);
        assert_eq!(
            parse("Edit", Some(&edit)),
            ToolInput::Edit {
                path: "/a/b.rs".into(),
                before: "x".into(),
                after: "y".into()
            }
        );
        let ask = object(
            r#"{"questions":[{"question":"Which store?","header":"Store","multiSelect":false,"options":[{"label":"Postgres","description":"durable"},{"label":"SQLite"}]}]}"#,
        );
        let ToolInput::Ask(asked) = parse("AskUserQuestion", Some(&ask)) else {
            unreachable!("AskUserQuestion parses as a question");
        };
        assert_eq!(asked[0].header.as_deref(), Some("Store"));
        assert_eq!(asked[0].options[0].description.as_deref(), Some("durable"));
        let flat = object(r#"{"question":"Ship it?","choices":["Yes","No"]}"#);
        assert!(
            matches!(parse("ask_user", Some(&flat)), ToolInput::Ask(asked) if asked[0].options.len() == 2)
        );
        let odd = object(r#"{"server":"rivet","limit":3}"#);
        assert!(matches!(parse("mcp__x__y", Some(&odd)), ToolInput::Fields(fields) if fields.len() == 2));
    }

    #[test]
    fn a_failed_command_keeps_its_exit_code() {
        assert_eq!(output("Bash", "Exit code 101\nerror", true).exit_code, Some(101));
        assert_eq!(output("Bash", "fine", false).exit_code, None);
    }
}

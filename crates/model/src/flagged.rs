//! Background jobs that kept failing until memory stopped retrying them, set aside for a person to
//! retry or dismiss.

/// One flagged job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlaggedJob {
    /// The node's id for the job, which a retry or dismiss names.
    pub id: i64,
    /// What the job was for, when its task names it, such as a conversation.
    pub key: Option<String>,
    /// What the job worked on in words, when rh can name it: a conversation's or a page's title.
    pub subject: Option<String>,
    /// The job's input as compact JSON, empty when it had none.
    pub payload: String,
    /// The last error, in full.
    pub error: String,
    pub failures: u32,
    pub last_failed_ms: i64,
}

impl FlaggedJob {
    /// What the job worked on: its key, else its input.
    pub fn subject(&self) -> Option<&str> {
        self.key
            .as_deref()
            .filter(|key| !key.is_empty())
            .or_else(|| (!self.payload.is_empty()).then_some(self.payload.as_str()))
    }
}

/// The flagged jobs of one task that failed for one cause, shown once, by the newest of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlaggedGroup {
    pub task: String,
    /// The error's first line, with what varies from job to job, such as ids and numbers, masked.
    pub cause: String,
    pub count: u64,
    pub sample: FlaggedJob,
}

impl FlaggedGroup {
    /// The group in words: what its task does, and to what when rh names it, such as
    /// "Summarizing · Fix the flaky gate".
    pub fn title(&self) -> String {
        let doing = doing(&self.task);
        match self
            .sample
            .subject
            .as_deref()
            .map(str::trim)
            .filter(|subject| !subject.is_empty())
        {
            Some(subject) => format!("{doing} · {subject}"),
            None => doing,
        }
    }
}

/// What a background task does, in words; a task this app doesn't know reads as its name.
fn doing(task: &str) -> String {
    let known = match task {
        "compact-conversation" => "Summarizing",
        "extract-wiki" => "Writing wiki notes",
        "embed-target" => "Embedding",
        "synthesize-tool-call" => "Describing a tool call",
        "rivetos-heartbeat" => "Heartbeat",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_owned();
    }
    let words = task.replace(['-', '_'], " ");
    let mut letters = words.chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
}

/// What a person does with a flagged job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlaggedAction {
    /// Queue it to run again, from no failures.
    Retry,
    /// Set it aside for good.
    Dismiss,
}

#[cfg(test)]
mod tests {
    use super::{FlaggedGroup, FlaggedJob};

    fn group(task: &str, subject: Option<&str>) -> FlaggedGroup {
        FlaggedGroup {
            task: task.to_owned(),
            cause: "provider request failed".to_owned(),
            count: 1,
            sample: FlaggedJob {
                id: 1,
                key: None,
                subject: subject.map(str::to_owned),
                payload: String::new(),
                error: String::new(),
                failures: 5,
                last_failed_ms: 0,
            },
        }
    }

    #[test]
    fn a_group_reads_as_its_task_in_words_and_what_it_worked_on() {
        assert_eq!(
            group("compact-conversation", Some("Fix the flaky gate")).title(),
            "Summarizing · Fix the flaky gate"
        );
        assert_eq!(group("embed-target", Some(" ")).title(), "Embedding");
        assert_eq!(group("reap-dead-jobs", None).title(), "Reap dead jobs");
    }
}

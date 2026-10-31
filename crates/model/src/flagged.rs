//! Background jobs that kept failing until memory stopped retrying them, set aside for a person to
//! retry or dismiss.

/// One flagged job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlaggedJob {
    /// The node's id for the job, which a retry or dismiss names.
    pub id: i64,
    /// What the job was for, when its task names it, such as a conversation.
    pub key: Option<String>,
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

/// What a person does with a flagged job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlaggedAction {
    /// Queue it to run again, from no failures.
    Retry,
    /// Set it aside for good.
    Dismiss,
}

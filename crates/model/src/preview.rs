use std::collections::HashMap;

use crate::SessionId;

/// What a session is doing now, or last said, in one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub kind: PreviewKind,
    pub text: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    /// A turn is running: the tool in progress and the gist of its input, or the line being written.
    Running,
    /// The session put a question to the person.
    Asking,
    /// A tool is waiting on the person's permission.
    Approval,
    /// Idle: the last line of its last reply.
    Said,
}

impl PreviewKind {
    /// Whether the preview is the session waiting on the person.
    pub fn waits(self) -> bool {
        matches!(self, Self::Asking | Self::Approval)
    }
}

pub type Previews = HashMap<SessionId, Preview>;

/// What a session started in a topic right now would be told, section by section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartupPreview {
    pub sections: Vec<StartupPart>,
    pub tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupPart {
    pub title: String,
    pub text: String,
}

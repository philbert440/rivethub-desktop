//! The sessions rh counts as obvious trash, and what deleting a session did.

use crate::SessionId;

/// Why rh counts a session as obvious trash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashReason {
    /// Nothing was said in it, and its title names nothing.
    Empty,
    /// One short prompt, answered with a single word or not at all.
    OneWordReply,
}

/// A session rh counts as obvious trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trash {
    pub session: SessionId,
    pub title: Option<String>,
    pub updated_at_ms: i64,
    pub reason: TrashReason,
    /// The one word a one-word reply was.
    pub reply: Option<String>,
    /// The node that counted it; `None` is this client's own node.
    pub node: Option<String>,
}

impl Trash {
    /// Why it is trash, in words.
    pub fn why(&self) -> String {
        match (self.reason, &self.reply) {
            (TrashReason::Empty, _) => "Nothing was said in it".to_owned(),
            (TrashReason::OneWordReply, Some(reply)) => format!("One prompt, answered “{reply}”"),
            (TrashReason::OneWordReply, None) => "One prompt, never answered".to_owned(),
        }
    }
}

/// What a delete did to one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deletion {
    Deleted,
    /// A chat, terminal or summary is running in it, so rh left it alone.
    Live,
    /// The node knows nothing of it.
    NotFound,
    Failed,
}

impl Deletion {
    /// Why a session stayed, or `None` once it is gone.
    pub fn refusal(self) -> Option<&'static str> {
        match self {
            Self::Deleted => None,
            Self::Live => Some("a turn or a terminal is still running in it"),
            Self::NotFound => Some("its node knows nothing of it"),
            Self::Failed => Some("its node couldn't delete it"),
        }
    }
}

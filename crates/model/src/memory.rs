use crate::SessionId;

/// How far back a memory search reaches. rh resolves each period in the node's local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    #[default]
    AnyTime,
    Today,
    Yesterday,
    LastWeek,
    LastTwoWeeks,
}

impl Period {
    pub const ALL: [Self; 5] = [
        Self::AnyTime,
        Self::Today,
        Self::Yesterday,
        Self::LastWeek,
        Self::LastTwoWeeks,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::AnyTime => "Any time",
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::LastWeek => "Last 7 days",
            Self::LastTwoWeeks => "Last 14 days",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Message,
    Summary,
}

/// One memory search hit. rh names the session for messages but not for summaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub source: Source,
    pub session: Option<SessionId>,
    pub text: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagChange {
    Add,
    Remove,
}

/// A tag the tagger wants to add to, or take off, one conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagProposal {
    pub id: String,
    pub tag: String,
    pub change: TagChange,
    pub conversation: String,
    pub reason: String,
}

/// A new tag value the tagger wants in the vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermProposal {
    pub key: String,
    pub value: String,
    pub display: String,
    pub reason: String,
}

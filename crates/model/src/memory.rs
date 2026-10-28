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

/// One memory search hit, with the session it came from when rh knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub source: Source,
    pub session: Option<SessionId>,
    pub text: String,
    pub at_ms: i64,
}

impl Found {
    /// A harness's private reasoning, which rh stores alongside what was said.
    fn is_reasoning(&self) -> bool {
        self.text.trim_start().starts_with("[thinking]")
    }
}

/// What a person searching their sessions wants to see: what was said, each thing once.
pub fn said(hits: Vec<Found>) -> Vec<Found> {
    let mut seen = std::collections::HashSet::new();
    hits.into_iter()
        .filter(|hit| !hit.is_reasoning())
        .filter(|hit| seen.insert((hit.session.clone(), hit.text.clone())))
        .collect()
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

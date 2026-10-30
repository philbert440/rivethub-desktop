//! Changes to the topic, entity and type trees that wait for a person, and how memory is keeping up.

mod tree;

pub use tree::{Branch, Row};

use crate::{SessionId, TopicId};

/// One change to a tree, naming topics, entities and types by id, or to a wiki page by slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Merge {
        source: TopicId,
        into: TopicId,
    },
    Move {
        topic: TopicId,
        parent: Option<TopicId>,
    },
    Rename {
        topic: TopicId,
        name: String,
    },
    Close {
        topic: TopicId,
    },
    Delete {
        topic: TopicId,
    },
    /// A new work topic, with the id it will get, its other names, and what it would file.
    CreateTopic {
        topic: TopicId,
        name: String,
        parent: Option<TopicId>,
        aliases: Vec<String>,
        sessions: Vec<SessionId>,
        conversations: Vec<String>,
    },
    /// Sessions and conversations filed under a topic that exists already.
    FileInto {
        topic: TopicId,
        sessions: Vec<SessionId>,
        conversations: Vec<String>,
    },
    AddType {
        topic: TopicId,
        name: String,
        parent: Option<TopicId>,
    },
    /// One wiki page folded into another.
    MergePage {
        page: String,
        into: String,
    },
    /// A loose wiki page removed, with its file.
    RemovePage {
        page: String,
    },
}

impl Change {
    /// The topic or type this change creates, which later changes in its batch may sit under.
    pub fn creates(&self) -> Option<&TopicId> {
        match self {
            Self::CreateTopic { topic, .. } | Self::AddType { topic, .. } => Some(topic),
            _ => None,
        }
    }

    /// Where a new topic or type goes.
    fn under(&self) -> Option<&TopicId> {
        match self {
            Self::CreateTopic { parent, .. } | Self::AddType { parent, .. } => parent.as_ref(),
            _ => None,
        }
    }

    /// The topic that exists already that this change acts on.
    fn subject(&self) -> Option<&TopicId> {
        match self {
            Self::Merge { source: topic, .. }
            | Self::Move { topic, .. }
            | Self::Rename { topic, .. }
            | Self::Close { topic }
            | Self::Delete { topic }
            | Self::FileInto { topic, .. } => Some(topic),
            Self::CreateTopic { .. }
            | Self::AddType { .. }
            | Self::MergePage { .. }
            | Self::RemovePage { .. } => None,
        }
    }

    /// The sessions and conversations this change would file.
    pub fn members(&self) -> Option<(&[SessionId], &[String])> {
        match self {
            Self::CreateTopic {
                sessions,
                conversations,
                ..
            }
            | Self::FileInto {
                sessions,
                conversations,
                ..
            } => Some((sessions, conversations)),
            _ => None,
        }
    }

    /// Whether this changes a wiki page rather than a tree.
    pub fn is_page(&self) -> bool {
        matches!(self, Self::MergePage { .. } | Self::RemovePage { .. })
    }
}

/// Who proposed a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proposer {
    Categorizer,
    Backfill,
}

/// One of the sessions or conversations behind a proposal, by id, with its title and when it was
/// last active when the proposal says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub id: String,
    pub title: Option<String>,
    pub last_active_ms: Option<i64>,
}

/// A proposed change, why, and the sessions behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub id: String,
    pub change: Change,
    pub reason: String,
    pub sessions: u64,
    /// A few of the sessions.
    pub samples: Vec<Sample>,
}

/// The proposals one run made together, waiting to be approved or rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub id: String,
    pub by: Proposer,
    pub at_ms: i64,
    pub proposals: Vec<Proposal>,
}

impl Batch {
    /// How many sessions and conversations the batch would file, into new topics and existing ones.
    pub fn sessions(&self) -> u64 {
        self.proposals
            .iter()
            .filter(|proposal| proposal.change.members().is_some())
            .map(|proposal| proposal.sessions)
            .sum()
    }

    /// The wiki page changes, which sit outside the tree.
    pub fn pages(&self) -> impl Iterator<Item = &Proposal> {
        self.proposals.iter().filter(|proposal| proposal.change.is_page())
    }
}

/// How conversations are filed: by the categorizer, by you, not yet, or never (test runs and the like).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Filing {
    pub categorizer: u64,
    pub you: u64,
    pub uncategorized: u64,
    pub left_out: u64,
    pub corrections_last_week: u64,
}

/// An entity type and how many entities sit directly under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeCount {
    pub kind: TopicId,
    pub name: String,
    pub entities: u64,
}

/// The wiki's pages: those topics own, entities by type, and those nothing owns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wiki {
    pub topic_pages: u64,
    pub entity_types: Vec<TypeCount>,
    pub loose_pages: u64,
}

/// When a worker last finished a job, and how many of its jobs are failing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Worker {
    pub last_finished_ms: Option<i64>,
    pub failing: u64,
}

/// The background workers that keep memory up to date.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Workers {
    pub embedder: Worker,
    pub summarizer: Worker,
    pub categorizer: Worker,
    pub wiki_builder: Worker,
}

/// How memory is keeping up: filing, the wiki, the workers, and the proposals waiting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Upkeep {
    pub filing: Filing,
    pub wiki: Wiki,
    pub workers: Workers,
    pub pending: u64,
}

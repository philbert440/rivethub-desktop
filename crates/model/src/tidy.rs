//! Changes to the topic, entity and type trees that wait for a person, and how memory is keeping up.

use std::collections::HashSet;

use crate::{SessionId, TopicId};

/// One change to a tree, naming topics, entities and types by id.
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
    /// A new work topic, with the id it will get, and what it would file.
    CreateTopic {
        topic: TopicId,
        name: String,
        parent: Option<TopicId>,
        sessions: Vec<SessionId>,
        conversations: Vec<String>,
    },
    AddType {
        topic: TopicId,
        name: String,
        parent: Option<TopicId>,
    },
}

impl Change {
    /// The topic or type this change creates, which later changes in its batch may sit under.
    pub fn creates(&self) -> Option<&TopicId> {
        match self {
            Self::CreateTopic { topic, .. } | Self::AddType { topic, .. } => Some(topic),
            Self::Merge { .. }
            | Self::Move { .. }
            | Self::Rename { .. }
            | Self::Close { .. }
            | Self::Delete { .. } => None,
        }
    }

    /// Where a new topic or type goes.
    fn under(&self) -> Option<&TopicId> {
        match self {
            Self::CreateTopic { parent, .. } | Self::AddType { parent, .. } => parent.as_ref(),
            Self::Merge { .. }
            | Self::Move { .. }
            | Self::Rename { .. }
            | Self::Close { .. }
            | Self::Delete { .. } => None,
        }
    }
}

/// Who proposed a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proposer {
    Categorizer,
    Backfill,
}

/// A proposed change, why, and the sessions behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub id: String,
    pub change: Change,
    pub reason: String,
    pub sessions: u64,
    /// A few of the sessions' titles.
    pub samples: Vec<String>,
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
    /// The batch as a proposed tree: each new topic or type with the proposals that would sit under
    /// it right below, one level deeper, and everything else at the top in the order rh gave.
    pub fn tree(&self) -> Vec<(usize, &Proposal)> {
        let created: HashSet<&TopicId> = self
            .proposals
            .iter()
            .filter_map(|proposal| proposal.change.creates())
            .collect();
        let mut out = Vec::with_capacity(self.proposals.len());
        let mut placed: HashSet<&str> = HashSet::new();
        let tops = self.proposals.iter().filter(|proposal| {
            proposal
                .change
                .under()
                .is_none_or(|parent| !created.contains(parent))
        });
        for top in tops {
            self.place(top, 0, &mut out, &mut placed);
        }
        for proposal in &self.proposals {
            self.place(proposal, 0, &mut out, &mut placed);
        }
        out
    }

    fn place<'a>(
        &'a self,
        proposal: &'a Proposal,
        depth: usize,
        out: &mut Vec<(usize, &'a Proposal)>,
        placed: &mut HashSet<&'a str>,
    ) {
        if !placed.insert(proposal.id.as_str()) {
            return;
        }
        out.push((depth, proposal));
        let Some(created) = proposal.change.creates() else {
            return;
        };
        for child in self
            .proposals
            .iter()
            .filter(|child| child.change.under() == Some(created))
        {
            self.place(child, depth + 1, out, placed);
        }
    }

    /// How many sessions the batch would file, across its new topics.
    pub fn sessions(&self) -> u64 {
        self.proposals
            .iter()
            .filter(|proposal| matches!(proposal.change, Change::CreateTopic { .. }))
            .map(|proposal| proposal.sessions)
            .sum()
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

#[cfg(test)]
mod tests {
    use super::{Batch, Change, Proposal, Proposer};
    use crate::TopicId;

    fn id(text: &str) -> TopicId {
        TopicId::new(text.to_owned())
    }

    fn proposal(key: &str, change: Change) -> Proposal {
        Proposal {
            id: key.to_owned(),
            change,
            reason: String::new(),
            sessions: 0,
            samples: Vec::new(),
        }
    }

    fn create(key: &str, topic: &str, parent: Option<&str>) -> Proposal {
        proposal(
            key,
            Change::CreateTopic {
                topic: id(topic),
                name: topic.to_owned(),
                parent: parent.map(id),
                sessions: Vec::new(),
                conversations: Vec::new(),
            },
        )
    }

    #[test]
    fn new_topics_nest_under_the_new_topics_they_sit_in() {
        let batch = Batch {
            id: "b".to_owned(),
            by: Proposer::Backfill,
            at_ms: 0,
            proposals: vec![
                create("child", "rail", Some("desktop")),
                proposal(
                    "rename",
                    Change::Rename {
                        topic: id("old"),
                        name: "New".to_owned(),
                    },
                ),
                create("root", "desktop", None),
                create("grandchild", "dots", Some("rail")),
                create("existing", "certs", Some("mesh")),
            ],
        };
        let tree: Vec<(usize, &str)> = batch
            .tree()
            .into_iter()
            .map(|(depth, proposal)| (depth, proposal.id.as_str()))
            .collect();
        assert_eq!(
            tree,
            [
                (0, "rename"),
                (0, "root"),
                (1, "child"),
                (2, "grandchild"),
                (0, "existing")
            ]
        );
    }
}

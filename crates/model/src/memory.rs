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

/// Whether rh has embedded a stored part, which meaning-based search needs to find it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vector {
    Embedded,
    Pending,
    Failed,
    Skipped,
}

impl Vector {
    pub fn label(self) -> &'static str {
        match self {
            Self::Embedded => "Embedded",
            Self::Pending => "Not embedded yet",
            Self::Failed => "Embedding failed",
            Self::Skipped => "Not embeddable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryKind {
    Leaf,
    Branch,
    Root,
}

impl SummaryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Leaf => "Leaf",
            Self::Branch => "Branch",
            Self::Root => "Root",
        }
    }
}

/// A summary memory wrote of a session: a leaf over a batch of messages, or a branch or root
/// over the summaries that roll up into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub id: String,
    pub parent: Option<String>,
    pub kind: SummaryKind,
    pub text: String,
    pub messages: u32,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub vector: Vector,
}

/// A session's messages in memory, by who sent them and whether each is embedded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MessageTally {
    pub total: u32,
    pub user: u32,
    pub assistant: u32,
    pub system: u32,
    pub tool: u32,
    pub embedded: u32,
    pub pending: u32,
    pub failed: u32,
    pub skipped: u32,
}

/// What memory holds of one session.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionMemory {
    pub summaries: Vec<Summary>,
    pub messages: MessageTally,
}

impl SessionMemory {
    /// The summary of the session's latest messages: the one ending last, a leaf over a rollup that
    /// ends at the same time.
    pub fn latest(&self) -> Option<&Summary> {
        self.summaries
            .iter()
            .max_by_key(|summary| (summary.to_ms, summary.kind == SummaryKind::Leaf))
    }

    /// The summaries as a tree, with each one's depth: every summary sits under the one it rolls
    /// up into, and siblings run earliest first. A leaf not rolled up yet sits at the top level.
    pub fn outline(&self) -> Vec<(usize, &Summary)> {
        let known: std::collections::HashSet<&str> =
            self.summaries.iter().map(|summary| summary.id.as_str()).collect();
        let mut under: std::collections::HashMap<Option<&str>, Vec<&Summary>> =
            std::collections::HashMap::new();
        for summary in &self.summaries {
            let parent = summary.parent.as_deref().filter(|parent| known.contains(parent));
            under.entry(parent).or_default().push(summary);
        }
        for siblings in under.values_mut() {
            siblings.sort_by_key(|summary| (summary.from_ms.or(summary.to_ms), summary.id.as_str()));
        }
        let mut stack: Vec<(usize, &Summary)> = under
            .get(&None)
            .map(|top| top.iter().rev().map(|summary| (0, *summary)).collect())
            .unwrap_or_default();
        let mut outline = Vec::with_capacity(self.summaries.len());
        while let Some((depth, summary)) = stack.pop() {
            outline.push((depth, summary));
            if let Some(children) = under.get(&Some(summary.id.as_str())) {
                stack.extend(children.iter().rev().map(|child| (depth + 1, *child)));
            }
        }
        outline
    }

    pub fn embedded_summaries(&self) -> usize {
        self.summaries
            .iter()
            .filter(|summary| summary.vector == Vector::Embedded)
            .count()
    }

    pub fn summaries_of(&self, kind: SummaryKind) -> usize {
        self.summaries
            .iter()
            .filter(|summary| summary.kind == kind)
            .count()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str, parent: Option<&str>, kind: SummaryKind, from_ms: i64) -> Summary {
        Summary {
            id: id.to_owned(),
            parent: parent.map(str::to_owned),
            kind,
            text: String::new(),
            messages: 0,
            from_ms: Some(from_ms),
            to_ms: Some(from_ms),
            vector: Vector::Embedded,
        }
    }

    #[test]
    fn the_latest_summary_ends_last_and_a_leaf_beats_its_rollup() {
        let memory = SessionMemory {
            summaries: vec![
                summary("early", None, SummaryKind::Leaf, 10),
                summary("root", None, SummaryKind::Root, 40),
                summary("last", Some("root"), SummaryKind::Leaf, 40),
            ],
            messages: MessageTally::default(),
        };
        assert_eq!(memory.latest().map(|summary| summary.id.as_str()), Some("last"));
        assert!(SessionMemory::default().latest().is_none());
    }

    #[test]
    fn outline_nests_leaves_under_their_branch_in_time_order() {
        let memory = SessionMemory {
            summaries: vec![
                summary("late-leaf", None, SummaryKind::Leaf, 50),
                summary("b", None, SummaryKind::Branch, 10),
                summary("leaf-2", Some("b"), SummaryKind::Leaf, 20),
                summary("leaf-1", Some("b"), SummaryKind::Leaf, 10),
                summary("orphan", Some("gone"), SummaryKind::Leaf, 30),
            ],
            messages: MessageTally::default(),
        };
        let outline: Vec<(usize, &str)> = memory
            .outline()
            .into_iter()
            .map(|(depth, summary)| (depth, summary.id.as_str()))
            .collect();
        assert_eq!(
            outline,
            [
                (0, "b"),
                (1, "leaf-1"),
                (1, "leaf-2"),
                (0, "orphan"),
                (0, "late-leaf")
            ]
        );
    }
}

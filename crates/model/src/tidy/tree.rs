//! A batch as the work tree it proposes: new topics where they would go, the topics that exist
//! already shown for what the batch does in or under them, and each change under the topic it
//! changes.

use std::collections::{HashMap, HashSet};

use super::{Batch, Change, Proposal};
use crate::TopicId;
use crate::topics::Topic;

/// What a row of the proposed tree shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Branch<'a> {
    /// A topic that exists already, there for what sits in or under it.
    Topic(&'a TopicId),
    /// A proposal: a new topic or type heads its own branch, any other change sits under the topic
    /// it changes.
    Proposal(&'a Proposal),
}

/// One row of the proposed tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row<'a> {
    pub depth: usize,
    pub branch: Branch<'a>,
    /// The topic this row heads, when it heads one: rows below it at greater depth sit in it.
    pub heads: Option<&'a TopicId>,
}

struct Plan<'a> {
    batch: &'a Batch,
    names: HashMap<&'a TopicId, &'a str>,
    parents: HashMap<&'a TopicId, Option<&'a TopicId>>,
    created: HashMap<&'a TopicId, &'a Proposal>,
    nodes: HashSet<&'a TopicId>,
}

impl<'a> Plan<'a> {
    fn new(batch: &'a Batch, existing: &'a [Topic]) -> Self {
        let created: HashMap<&TopicId, &Proposal> = batch
            .proposals
            .iter()
            .filter_map(|proposal| proposal.change.creates().map(|topic| (topic, proposal)))
            .collect();
        let mut parents: HashMap<&TopicId, Option<&TopicId>> = existing
            .iter()
            .map(|topic| (&topic.id, topic.parent.as_ref()))
            .collect();
        let mut names: HashMap<&TopicId, &str> = existing
            .iter()
            .map(|topic| (&topic.id, topic.name.as_str()))
            .collect();
        for proposal in &batch.proposals {
            match &proposal.change {
                Change::Move { topic, parent } => {
                    parents.insert(topic, parent.as_ref());
                }
                Change::CreateTopic {
                    topic, name, parent, ..
                }
                | Change::AddType {
                    topic, name, parent, ..
                } => {
                    parents.insert(topic, parent.as_ref());
                    names.insert(topic, name);
                }
                _ => {}
            }
        }
        let mut nodes: HashSet<&TopicId> = created.keys().copied().collect();
        let mut queue: Vec<&TopicId> = batch
            .proposals
            .iter()
            .filter_map(|proposal| proposal.change.subject().or_else(|| proposal.change.under()))
            .chain(created.keys().copied())
            .collect();
        while let Some(topic) = queue.pop() {
            nodes.insert(topic);
            if let Some(parent) = parents.get(topic).copied().flatten()
                && !nodes.contains(parent)
            {
                queue.push(parent);
            }
        }
        Self {
            batch,
            names,
            parents,
            created,
            nodes,
        }
    }

    fn parent(&self, topic: &TopicId) -> Option<&'a TopicId> {
        self.parents
            .get(topic)
            .copied()
            .flatten()
            .filter(|parent| self.nodes.contains(parent))
    }

    /// The topics directly under `parent` (the roots for `None`): those that exist by name, then
    /// the new ones in the order proposed.
    fn children(&self, parent: Option<&TopicId>) -> Vec<&'a TopicId> {
        let mut existing: Vec<&TopicId> = self
            .nodes
            .iter()
            .copied()
            .filter(|topic| !self.created.contains_key(topic) && self.parent(topic) == parent)
            .collect();
        existing.sort_by_key(|topic| (self.names.get(topic).copied().unwrap_or_default(), topic.as_str()));
        let new = self
            .batch
            .proposals
            .iter()
            .filter_map(|proposal| proposal.change.creates())
            .filter(|topic| self.parent(topic) == parent);
        existing.into_iter().chain(new).collect()
    }

    fn place(
        &self,
        topic: &'a TopicId,
        depth: usize,
        out: &mut Vec<Row<'a>>,
        seen: &mut HashSet<&'a TopicId>,
    ) {
        if !seen.insert(topic) {
            return;
        }
        let branch = self
            .created
            .get(topic)
            .map_or(Branch::Topic(topic), |proposal| Branch::Proposal(proposal));
        out.push(Row {
            depth,
            branch,
            heads: Some(topic),
        });
        out.extend(
            self.batch
                .proposals
                .iter()
                .filter(|proposal| proposal.change.subject() == Some(topic))
                .map(|proposal| Row {
                    depth: depth + 1,
                    branch: Branch::Proposal(proposal),
                    heads: None,
                }),
        );
        for child in self.children(Some(topic)) {
            self.place(child, depth + 1, out, seen);
        }
    }
}

impl Batch {
    /// The batch as the work tree it proposes, given the topics that exist now. Wiki page changes
    /// sit outside it, in `pages`.
    pub fn tree<'a>(&'a self, existing: &'a [Topic]) -> Vec<Row<'a>> {
        let plan = Plan::new(self, existing);
        let mut out = Vec::with_capacity(self.proposals.len() + plan.nodes.len());
        let mut seen = HashSet::new();
        for root in plan.children(None) {
            plan.place(root, 0, &mut out, &mut seen);
        }
        // A batch whose moves loop leaves topics no root reaches; they still show, at the top.
        let mut stranded: Vec<&TopicId> = plan
            .nodes
            .iter()
            .copied()
            .filter(|topic| !seen.contains(topic))
            .collect();
        stranded.sort_by_key(|topic| topic.as_str());
        for topic in stranded {
            plan.place(topic, 0, &mut out, &mut seen);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{Branch, Row};
    use crate::TopicId;
    use crate::tidy::{Batch, Change, Proposal, Proposer};
    use crate::topics::Topic;

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
                aliases: Vec::new(),
                sessions: Vec::new(),
                conversations: Vec::new(),
            },
        )
    }

    fn topic(key: &str, parent: Option<&str>) -> Topic {
        Topic {
            id: id(key),
            slug: key.to_owned(),
            name: key.to_owned(),
            parent: parent.map(id),
            last_active_ms: None,
            updated_ms: 0,
            closed_ms: None,
            directory: None,
        }
    }

    fn shape<'a>(rows: &[Row<'a>]) -> Vec<(usize, &'a str)> {
        rows.iter()
            .map(|row| {
                (
                    row.depth,
                    match row.branch {
                        Branch::Topic(topic) => topic.as_str(),
                        Branch::Proposal(proposal) => proposal.id.as_str(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn a_batch_reads_as_the_tree_it_proposes() {
        let existing = [
            topic("mesh", None),
            topic("certs", Some("mesh")),
            topic("old", None),
        ];
        let batch = Batch {
            id: "b".to_owned(),
            by: Proposer::Backfill,
            at_ms: 0,
            proposals: vec![
                create("rail", "rail", Some("desktop")),
                proposal(
                    "rename",
                    Change::Rename {
                        topic: id("old"),
                        name: "New".to_owned(),
                    },
                ),
                create("desktop", "desktop", None),
                create("dots", "dots", Some("rail")),
                create("renewal", "renewal", Some("certs")),
                proposal(
                    "file",
                    Change::FileInto {
                        topic: id("certs"),
                        sessions: Vec::new(),
                        conversations: Vec::new(),
                    },
                ),
                proposal(
                    "move",
                    Change::Move {
                        topic: id("old"),
                        parent: Some(id("desktop")),
                    },
                ),
                proposal(
                    "page",
                    Change::RemovePage {
                        page: "junk".to_owned(),
                    },
                ),
            ],
        };
        assert_eq!(
            shape(&batch.tree(&existing)),
            [
                (0, "mesh"),
                (1, "certs"),
                (2, "file"),
                (2, "renewal"),
                (0, "desktop"),
                (1, "old"),
                (2, "rename"),
                (2, "move"),
                (1, "rail"),
                (2, "dots"),
            ]
        );
        assert_eq!(
            batch.pages().map(|page| page.id.as_str()).collect::<Vec<_>>(),
            ["page"]
        );
    }

    #[test]
    fn moves_that_loop_still_show_every_change() {
        let existing = [topic("a", None), topic("b", None)];
        let batch = Batch {
            id: "b".to_owned(),
            by: Proposer::Categorizer,
            at_ms: 0,
            proposals: vec![
                proposal(
                    "a-under-b",
                    Change::Move {
                        topic: id("a"),
                        parent: Some(id("b")),
                    },
                ),
                proposal(
                    "b-under-a",
                    Change::Move {
                        topic: id("b"),
                        parent: Some(id("a")),
                    },
                ),
            ],
        };
        let mut shown: Vec<&str> = shape(&batch.tree(&existing))
            .into_iter()
            .map(|(_, key)| key)
            .collect();
        shown.sort_unstable();
        assert_eq!(shown, ["a", "a-under-b", "b", "b-under-a"]);
    }
}

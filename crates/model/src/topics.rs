use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, RandomState};

use crate::session::SessionSummary;
use crate::{SessionId, TopicId};

const MAX_DEPTH: usize = 8;

/// How many levels topics nest, a top-level topic being the first; rh refuses deeper.
pub const MAX_LEVELS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub id: TopicId,
    pub slug: String,
    pub name: String,
    pub parent: Option<TopicId>,
    pub last_active_ms: Option<i64>,
    /// When the topic itself last changed: created, renamed, moved, closed or reopened.
    pub updated_ms: i64,
    pub closed_ms: Option<i64>,
    pub directory: Option<Directory>,
}

/// Where a topic's sessions work: a path on one node, or a path every node shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Directory {
    Local { node: String, path: String },
    Shared { path: String },
}

impl Topic {
    /// Closed, and nothing in it has been active since.
    pub fn is_closed(&self, active_ms: i64) -> bool {
        self.closed_ms.is_some_and(|closed| active_ms <= closed)
    }
}

pub type Placements = HashMap<SessionId, TopicId>;

/// When each session was marked as history.
pub type HistoryMarks = HashMap<SessionId, i64>;

/// Who filed a session or conversation in its topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiledBy {
    You,
    Categorizer,
}

/// A conversation filed in a topic that is no harness session, such as a bot chat, a voice call or
/// an import; it opens in Memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledConversation {
    pub id: String,
    pub topic: TopicId,
    pub title: Option<String>,
    pub channel: String,
    pub last_active_ms: Option<i64>,
    pub by: FiledBy,
    /// The node that captured it, when rh recorded one.
    pub node: Option<String>,
}

/// A placed session as rh names it, and who filed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledSession {
    pub session: SessionId,
    pub topic: TopicId,
    pub title: Option<String>,
    pub last_active_ms: Option<i64>,
    pub by: FiledBy,
    /// The node that captured it, when rh recorded one.
    pub node: Option<String>,
}

impl FiledSession {
    /// The session as far as rh remembers it, for a session no node lists any more.
    pub fn summary(&self) -> SessionSummary {
        SessionSummary::remembered(
            self.session.clone(),
            self.title.clone(),
            self.last_active_ms.unwrap_or_default(),
            self.node.clone(),
        )
    }
}

/// What rh has filed in one topic: sessions, and conversations that are no harness session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TopicFiling {
    pub sessions: Vec<FiledSession>,
    pub conversations: Vec<FiledConversation>,
}

/// The hub's topics, where its sessions are placed, and which of them are marked as history.
#[derive(Debug, Clone, Default)]
pub struct Filing {
    pub topics: Vec<Topic>,
    pub placements: Placements,
    pub marks: HistoryMarks,
    /// Who filed each placed session.
    pub filed_by: HashMap<SessionId, FiledBy>,
    /// The placed sessions as rh remembers them, for those no node lists any more.
    pub remembered: Vec<SessionSummary>,
    pub conversations: Vec<FiledConversation>,
}

/// Marked as history, and not active since: it stays out of its topic's active sessions and out of
/// Uncategorized.
pub fn in_history(session: &SessionSummary, marks: &HistoryMarks) -> bool {
    marks
        .get(&session.id)
        .is_some_and(|marked| session.updated_at_ms <= *marked)
}

/// What decides whether a session is Active: the time now, how long a quiet session stays Active,
/// the sessions that are live (running, waiting on you, or open) and the history marks.
#[derive(Debug, Clone, Copy)]
pub struct Activity<'a, S = RandomState> {
    pub now_ms: i64,
    pub window_ms: i64,
    pub live: &'a HashSet<SessionId, S>,
    pub marks: &'a HistoryMarks,
}

impl<S: BuildHasher> Activity<'_, S> {
    /// Live, or busy within the window and not marked as history since. Everything else is History.
    pub fn is_active(&self, session: &SessionSummary) -> bool {
        self.live.contains(&session.id)
            || (self.now_ms - session.updated_at_ms <= self.window_ms && !in_history(session, self.marks))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub name: String,
    pub topic: Option<TopicId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card<'a> {
    pub topic: &'a Topic,
    /// Its Active sessions, newest first.
    pub sessions: Vec<&'a SessionSummary>,
    /// How many of its sessions are History.
    pub history: usize,
    pub active_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Board<'a> {
    pub recent: Vec<Card<'a>>,
    pub past: Vec<Card<'a>>,
    pub uncategorized: Vec<&'a SessionSummary>,
    pub history: Vec<&'a SessionSummary>,
}

#[derive(Debug, Clone, Copy)]
pub struct Windows {
    pub now_ms: i64,
    /// How long a quiet session stays Active.
    pub active_ms: i64,
    pub recent_ms: i64,
}

#[derive(Debug)]
pub struct Atlas<'a> {
    topics: HashMap<&'a TopicId, &'a Topic>,
    sessions: HashMap<&'a SessionId, &'a SessionSummary>,
    placements: &'a Placements,
}

impl<'a> Atlas<'a> {
    pub fn new(topics: &'a [Topic], sessions: &'a [SessionSummary], placements: &'a Placements) -> Self {
        Self {
            topics: topics.iter().map(|topic| (&topic.id, topic)).collect(),
            sessions: sessions.iter().map(|session| (&session.id, session)).collect(),
            placements,
        }
    }

    pub fn topic_of(&self, session: &SessionSummary) -> Option<&'a TopicId> {
        let mut current = session;
        for _ in 0..MAX_DEPTH {
            if let Some(topic) = self.placements.get(&current.id) {
                return self.topics.get(topic).map(|topic| &topic.id);
            }
            current = self.sessions.get(current.parent.as_ref()?)?;
        }
        None
    }

    pub fn root(&self, topic: &'a TopicId) -> &'a TopicId {
        let mut current = topic;
        for _ in 0..MAX_DEPTH {
            match self.topics.get(current).and_then(|topic| topic.parent.as_ref()) {
                Some(parent) if self.topics.contains_key(parent) => current = parent,
                _ => break,
            }
        }
        current
    }

    pub fn within(&self, session: &SessionSummary, topic: &TopicId) -> bool {
        self.topic_of(session)
            .is_some_and(|own| self.lineage(own).contains(&topic))
    }

    /// The topic and the topics above it, nearest first.
    pub fn lineage(&self, topic: &'a TopicId) -> Vec<&'a TopicId> {
        let mut lineage = vec![topic];
        let mut current = topic;
        for _ in 0..MAX_DEPTH {
            match self.topics.get(current).and_then(|topic| topic.parent.as_ref()) {
                Some(parent) if self.topics.contains_key(parent) => {
                    lineage.push(parent);
                    current = parent;
                }
                _ => break,
            }
        }
        lineage
    }

    /// Where sessions in the topic work: its own directory, else the nearest one above it.
    pub fn directory(&self, topic: &'a TopicId) -> Option<&'a Directory> {
        self.lineage(topic)
            .into_iter()
            .find_map(|id| self.topics.get(id).and_then(|known| known.directory.as_ref()))
    }

    /// How many levels the topic's subtree spans, the topic itself being one.
    pub fn height(&self, topic: &TopicId) -> usize {
        self.height_below(topic, MAX_DEPTH)
    }

    fn height_below(&self, topic: &TopicId, budget: usize) -> usize {
        let children = self
            .topics
            .values()
            .filter(|child| child.parent.as_ref() == Some(topic));
        1 + match budget {
            0 => 0,
            _ => children
                .map(|child| self.height_below(&child.id, budget - 1))
                .max()
                .unwrap_or(0),
        }
    }

    /// The topic's own subtopics, by name.
    pub fn children(&self, topic: &TopicId) -> Vec<&'a Topic> {
        let mut children: Vec<&'a Topic> = self
            .topics
            .values()
            .copied()
            .filter(|child| child.parent.as_ref() == Some(topic))
            .collect();
        children.sort_by_key(|child| (child.name.to_lowercase(), child.id.clone()));
        children
    }

    /// The sessions filed anywhere under the topic, its own included, leaving out subagents whose
    /// parent is under it too: they go with their parent.
    pub fn sessions_within(&self, topic: &TopicId) -> Vec<&'a SessionSummary> {
        self.sessions
            .values()
            .copied()
            .filter(|session| {
                self.within(session, topic)
                    && !session
                        .parent
                        .as_ref()
                        .and_then(|parent| self.sessions.get(parent))
                        .is_some_and(|parent| self.within(parent, topic))
            })
            .collect()
    }

    /// Whether `topic` can move under `parent`: never under itself or its own subtopics, and never
    /// so that its deepest subtopic would sit below `MAX_LEVELS`.
    pub fn can_nest(&self, topic: &TopicId, parent: &'a TopicId) -> bool {
        let lineage = self.lineage(parent);
        !lineage.contains(&topic) && lineage.len() + self.height(topic) <= MAX_LEVELS
    }
}

pub fn suggest<'a>(topics: &'a [Topic], query: &str, limit: usize) -> Vec<&'a Topic> {
    let wanted = query.trim().to_lowercase();
    let mut ranked: Vec<(u8, Reverse<i64>, &Topic)> = topics
        .iter()
        .filter_map(|topic| {
            let name = topic.name.to_lowercase();
            let rank = if wanted.is_empty() {
                2
            } else if name.starts_with(&wanted) || topic.slug.starts_with(&wanted) {
                0
            } else if name.contains(&wanted) || topic.slug.contains(&wanted) {
                1
            } else {
                return None;
            };
            Some((rank, Reverse(topic.last_active_ms.unwrap_or(0)), topic))
        })
        .collect();
    ranked.sort_by_key(|entry| (entry.0, entry.1));
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, _, topic)| topic)
        .collect()
}

/// Every topic as an indented outline: root topics, most recently active first, each followed by
/// its subtopics. A topic whose parent is unknown counts as a root.
pub fn outline(topics: &[Topic]) -> Vec<(usize, &Topic)> {
    let known: HashMap<&TopicId, &Topic> = topics.iter().map(|topic| (&topic.id, topic)).collect();
    let mut children: HashMap<Option<&TopicId>, Vec<&Topic>> = HashMap::new();
    for topic in topics {
        let parent = topic.parent.as_ref().filter(|parent| known.contains_key(parent));
        children.entry(parent).or_default().push(topic);
    }
    for siblings in children.values_mut() {
        siblings.sort_by(|a, b| {
            b.last_active_ms
                .cmp(&a.last_active_ms)
                .then_with(|| a.name.cmp(&b.name))
        });
    }
    let mut rows = Vec::with_capacity(topics.len());
    let mut stack: Vec<(usize, &Topic)> = children
        .get(&None)
        .map(|roots| roots.iter().rev().map(|topic| (0, *topic)).collect())
        .unwrap_or_default();
    while let Some((depth, topic)) = stack.pop() {
        rows.push((depth, topic));
        if depth < MAX_DEPTH
            && let Some(below) = children.get(&Some(&topic.id))
        {
            stack.extend(below.iter().rev().map(|child| (depth + 1, *child)));
        }
    }
    rows
}

pub fn board<'a, S: BuildHasher>(
    topics: &'a [Topic],
    sessions: &'a [SessionSummary],
    placements: &'a Placements,
    marks: &HistoryMarks,
    live: &HashSet<SessionId, S>,
    windows: Windows,
) -> Board<'a> {
    let activity = Activity {
        now_ms: windows.now_ms,
        window_ms: windows.active_ms,
        live,
        marks,
    };
    let atlas = Atlas::new(topics, sessions, placements);
    let mut filed: HashMap<&TopicId, Vec<&SessionSummary>> = HashMap::new();
    let mut board = Board::default();
    for session in sessions {
        if let Some(topic) = atlas.topic_of(session) {
            if session.parent.is_none() || atlas.topic_of_parent(session) != Some(topic) {
                filed.entry(atlas.root(topic)).or_default().push(session);
            }
        } else {
            let origin = atlas.origin(session);
            if !activity.is_active(origin) {
                board.history.push(session);
            } else if origin.id == session.id {
                board.uncategorized.push(session);
            }
        }
    }
    for topic in topics.iter().filter(|topic| atlas.root(&topic.id) == &topic.id) {
        let mut sessions = filed.remove(&topic.id).unwrap_or_default();
        sessions.sort_by_key(|session| Reverse(session.updated_at_ms));
        let newest = sessions.first().map_or(0, |session| session.updated_at_ms);
        let active_ms = newest
            .max(topic.last_active_ms.unwrap_or(0))
            .max(topic.updated_ms);
        let (sessions, history): (Vec<_>, Vec<_>) = sessions
            .into_iter()
            .partition(|session| activity.is_active(session));
        let card = Card {
            topic,
            sessions,
            history: history.len(),
            active_ms,
        };
        if !topic.is_closed(newest) && windows.now_ms - active_ms <= windows.recent_ms {
            board.recent.push(card);
        } else {
            board.past.push(card);
        }
    }
    board.recent.sort_by_key(|card| Reverse(card.active_ms));
    board.past.sort_by_key(|card| Reverse(card.active_ms));
    board
        .uncategorized
        .sort_by_key(|session| Reverse(session.updated_at_ms));
    board
        .history
        .sort_by_key(|session| Reverse(session.updated_at_ms));
    board
}

impl<'a> Atlas<'a> {
    /// The session a subagent was started from, or the session itself.
    pub fn origin(&self, session: &'a SessionSummary) -> &'a SessionSummary {
        let mut current = session;
        for _ in 0..MAX_DEPTH {
            match current
                .parent
                .as_ref()
                .and_then(|parent| self.sessions.get(parent))
            {
                Some(parent) => current = parent,
                None => break,
            }
        }
        current
    }

    fn topic_of_parent(&self, session: &SessionSummary) -> Option<&'a TopicId> {
        let parent = self.sessions.get(session.parent.as_ref()?)?;
        self.topic_of(parent)
    }
}

#[cfg(test)]
mod tests;

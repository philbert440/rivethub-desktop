use std::cmp::Reverse;
use std::collections::HashMap;

use crate::session::SessionSummary;
use crate::{SessionId, TopicId};

const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub id: TopicId,
    pub slug: String,
    pub name: String,
    pub parent: Option<TopicId>,
    pub last_active_ms: Option<i64>,
}

pub type Placements = HashMap<SessionId, TopicId>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub name: String,
    pub topic: Option<TopicId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card<'a> {
    pub topic: &'a Topic,
    pub sessions: Vec<&'a SessionSummary>,
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
    pub uncategorized_ms: i64,
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
            .is_some_and(|own| own == topic || self.root(own) == topic)
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

pub fn board<'a>(
    topics: &'a [Topic],
    sessions: &'a [SessionSummary],
    placements: &'a Placements,
    windows: Windows,
) -> Board<'a> {
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
            let young = windows.now_ms - origin.updated_at_ms <= windows.uncategorized_ms;
            if !young {
                board.history.push(session);
            } else if origin.id == session.id {
                board.uncategorized.push(session);
            }
        }
    }
    for topic in topics.iter().filter(|topic| atlas.root(&topic.id) == &topic.id) {
        let mut sessions = filed.remove(&topic.id).unwrap_or_default();
        sessions.sort_by_key(|session| Reverse(session.updated_at_ms));
        let newest = sessions.first().map(|session| session.updated_at_ms);
        let active_ms = newest.max(topic.last_active_ms).unwrap_or(0);
        let card = Card {
            topic,
            sessions,
            active_ms,
        };
        if windows.now_ms - active_ms <= windows.recent_ms {
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
    fn origin(&self, session: &'a SessionSummary) -> &'a SessionSummary {
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
mod tests {
    use super::*;
    use crate::session::SessionStatus;
    use crate::{HarnessId, NativeSessionId};

    const HOUR: i64 = 60 * 60 * 1000;
    const WINDOWS: Windows = Windows {
        now_ms: 100 * HOUR,
        uncategorized_ms: 24 * HOUR,
        recent_ms: 72 * HOUR,
    };

    fn id(native: &str) -> SessionId {
        SessionId::new(HarnessId::new("claude-code"), NativeSessionId::new(native))
    }

    fn session(native: &str, hours_ago: i64, parent: Option<&str>) -> SessionSummary {
        SessionSummary {
            id: id(native),
            title: None,
            status: SessionStatus::Idle,
            model: None,
            effort: None,
            cwd: None,
            updated_at_ms: WINDOWS.now_ms - hours_ago * HOUR,
            parent: parent.map(id),
            node: None,
            blocked: false,
        }
    }

    fn topic(slug: &str, parent: Option<&str>) -> Topic {
        Topic {
            id: TopicId::new(slug),
            slug: slug.into(),
            name: slug.into(),
            parent: parent.map(TopicId::new),
            last_active_ms: None,
        }
    }

    fn natives(sessions: &[&SessionSummary]) -> Vec<String> {
        sessions
            .iter()
            .map(|session| session.id.native().as_str().to_owned())
            .collect()
    }

    #[test]
    fn placed_sessions_file_under_their_root_topic_and_subagents_follow_their_parent() {
        let topics = [
            topic("desktop", None),
            topic("desktop-chat", Some("desktop")),
            topic("mesh", None),
        ];
        let sessions = [
            session("lead", 1, None),
            session("helper", 1, Some("lead")),
            session("chat", 2, None),
            session("certs", 200, None),
        ];
        let placements = Placements::from([
            (id("lead"), TopicId::new("desktop")),
            (id("chat"), TopicId::new("desktop-chat")),
            (id("certs"), TopicId::new("mesh")),
        ]);
        let board = board(&topics, &sessions, &placements, WINDOWS);
        assert_eq!(board.recent.len(), 1);
        assert_eq!(board.recent[0].topic.slug, "desktop");
        assert_eq!(natives(&board.recent[0].sessions), ["lead", "chat"]);
        assert_eq!(
            board
                .past
                .iter()
                .map(|card| card.topic.slug.as_str())
                .collect::<Vec<_>>(),
            ["mesh"]
        );
        assert!(board.uncategorized.is_empty() && board.history.is_empty());
    }

    #[test]
    fn a_side_track_placed_elsewhere_files_under_its_own_topic() {
        let topics = [topic("desktop", None), topic("mesh", None)];
        let sessions = [session("lead", 1, None), session("detour", 1, Some("lead"))];
        let placements = Placements::from([
            (id("lead"), TopicId::new("desktop")),
            (id("detour"), TopicId::new("mesh")),
        ]);
        let board = board(&topics, &sessions, &placements, WINDOWS);
        let mesh = board
            .recent
            .iter()
            .find(|card| card.topic.slug == "mesh")
            .map(|card| natives(&card.sessions));
        assert_eq!(mesh, Some(vec!["detour".to_owned()]));
    }

    #[test]
    fn unplaced_sessions_are_uncategorized_while_young_then_history() {
        let sessions = [
            session("fresh", 3, None),
            session("stale", 30, None),
            session("child", 3, Some("fresh")),
            session("old-child", 30, Some("stale")),
        ];
        let placements = Placements::new();
        let board = board(&[], &sessions, &placements, WINDOWS);
        assert_eq!(natives(&board.uncategorized), ["fresh"]);
        assert_eq!(natives(&board.history), ["stale", "old-child"]);
    }

    #[test]
    fn the_outline_puts_each_subtopic_under_its_root_and_the_busiest_root_first() {
        let mut quiet = topic("mesh", None);
        quiet.last_active_ms = Some(1);
        let mut busy = topic("desktop", None);
        busy.last_active_ms = Some(9);
        let topics = [
            topic("desktop-chat", Some("desktop")),
            quiet,
            topic("orphan", Some("gone")),
            busy,
        ];
        let rows: Vec<(usize, &str)> = outline(&topics)
            .into_iter()
            .map(|(depth, topic)| (depth, topic.slug.as_str()))
            .collect();
        assert_eq!(
            rows,
            [(0, "desktop"), (1, "desktop-chat"), (0, "mesh"), (0, "orphan")]
        );
    }

    #[test]
    fn suggestions_rank_prefix_matches_before_substrings_then_recency() {
        let mut quiet = topic("desktop-chat", None);
        quiet.last_active_ms = Some(1);
        let mut busy = topic("desktop", None);
        busy.last_active_ms = Some(9);
        let topics = [topic("rh-desktop", None), quiet, busy, topic("mesh", None)];
        let names = |query| {
            suggest(&topics, query, 5)
                .iter()
                .map(|topic| topic.slug.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("desk"), ["desktop", "desktop-chat", "rh-desktop"]);
        assert_eq!(names("zzz"), Vec::<String>::new());
    }
}

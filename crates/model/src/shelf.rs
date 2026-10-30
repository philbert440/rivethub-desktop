//! One step of a topic screen: the sessions under it, Active then History, and the harnesses they
//! usually run, for its quick start.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::hash::BuildHasher;

use crate::HarnessId;
use crate::TopicId;
use crate::session::SessionSummary;
use crate::topics::{Activity, Atlas};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shelf<'a> {
    /// Newest first.
    pub active: Vec<&'a SessionSummary>,
    /// Newest first.
    pub history: Vec<&'a SessionSummary>,
}

impl<'a> Shelf<'a> {
    pub fn of<S: BuildHasher>(atlas: &Atlas<'a>, step: &TopicId, activity: &Activity<'_, S>) -> Self {
        let mut sessions = atlas.sessions_within(step);
        sessions.sort_by_key(|session| (Reverse(session.updated_at_ms), session.id.clone()));
        let (active, history) = sessions
            .into_iter()
            .partition(|session| activity.is_active(session));
        Self { active, history }
    }

    pub fn len(&self) -> usize {
        self.active.len() + self.history.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The latest session of each harness these sessions ran, the most used harness first, then
    /// the most recently used: a quick start runs where that session ran.
    pub fn usual(&self, limit: usize) -> Vec<&'a SessionSummary> {
        let mut by_harness: HashMap<&HarnessId, (usize, &'a SessionSummary)> = HashMap::new();
        for session in self.active.iter().chain(&self.history).copied() {
            let entry = by_harness.entry(session.id.harness()).or_insert((0, session));
            entry.0 += 1;
            if session.updated_at_ms > entry.1.updated_at_ms {
                entry.1 = session;
            }
        }
        let mut usual: Vec<(usize, &'a SessionSummary)> = by_harness.into_values().collect();
        usual.sort_by_key(|(uses, latest)| (Reverse(*uses), Reverse(latest.updated_at_ms)));
        usual.into_iter().take(limit).map(|(_, latest)| latest).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::session::SessionStatus;
    use crate::topics::{HistoryMarks, Placements, Topic};
    use crate::{NativeSessionId, SessionId};

    const HOUR: i64 = 60 * 60 * 1000;
    const NOW: i64 = 100 * HOUR;

    fn id(harness: &str, native: &str) -> SessionId {
        SessionId::new(HarnessId::new(harness), NativeSessionId::new(native))
    }

    fn session(harness: &str, native: &str, hours_ago: i64, parent: Option<SessionId>) -> SessionSummary {
        SessionSummary {
            id: id(harness, native),
            title: None,
            status: SessionStatus::Idle,
            model: None,
            effort: None,
            cwd: None,
            updated_at_ms: NOW - hours_ago * HOUR,
            parent,
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
            updated_ms: 0,
            closed_ms: None,
            directory: None,
        }
    }

    #[test]
    fn a_step_holds_its_subtree_active_then_history_with_subagents_riding_along() {
        let topics = [
            topic("desk", None),
            topic("memory", Some("desk")),
            topic("mesh", None),
        ];
        let sessions = [
            session("claude-code", "lead", 1, None),
            session("claude-code", "helper", 1, Some(id("claude-code", "lead"))),
            session("grok-build", "search", 3, None),
            session("grok-build", "old", 50, None),
            session("codex", "elsewhere", 1, None),
            session("grok-build", "live", 90, None),
        ];
        let placements = Placements::from([
            (id("claude-code", "lead"), TopicId::new("desk")),
            (id("grok-build", "search"), TopicId::new("memory")),
            (id("grok-build", "old"), TopicId::new("memory")),
            (id("codex", "elsewhere"), TopicId::new("mesh")),
            (id("grok-build", "live"), TopicId::new("memory")),
        ]);
        let atlas = Atlas::new(&topics, &sessions, &placements);
        let live = HashSet::from([id("grok-build", "live")]);
        let marks = HistoryMarks::new();
        let activity = Activity {
            now_ms: NOW,
            window_ms: 24 * HOUR,
            live: &live,
            marks: &marks,
        };
        let natives = |list: &[&SessionSummary]| {
            list.iter()
                .map(|session| session.id.native().as_str().to_owned())
                .collect::<Vec<_>>()
        };
        let desk = Shelf::of(&atlas, &TopicId::new("desk"), &activity);
        assert_eq!(natives(&desk.active), ["lead", "search", "live"]);
        assert_eq!(natives(&desk.history), ["old"]);
        let memory = Shelf::of(&atlas, &TopicId::new("memory"), &activity);
        assert_eq!(memory.len(), 3);
        let usual: Vec<&str> = desk
            .usual(3)
            .iter()
            .map(|latest| latest.id.native().as_str())
            .collect();
        assert_eq!(usual, ["search", "lead"]);
        assert_eq!(
            atlas
                .children(&TopicId::new("desk"))
                .iter()
                .map(|child| child.slug.as_str())
                .collect::<Vec<_>>(),
            ["memory"]
        );
    }
}

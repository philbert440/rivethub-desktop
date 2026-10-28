use std::collections::{HashMap, HashSet};

use crate::SessionId;
use crate::session::SessionSummary;

const DEPTH_CAP: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub session: SessionSummary,
    pub depth: usize,
    pub children: usize,
    pub collapsed: bool,
}

pub fn nest<S: std::hash::BuildHasher>(
    sessions: &[SessionSummary],
    collapsed: &HashSet<SessionId, S>,
) -> Vec<Row> {
    let listed: HashSet<&SessionId> = sessions.iter().map(|session| &session.id).collect();
    let mut children: HashMap<&SessionId, Vec<&SessionSummary>> = HashMap::new();
    let mut roots: Vec<&SessionSummary> = Vec::new();
    for session in sessions {
        match session
            .parent
            .as_ref()
            .filter(|parent| listed.contains(parent) && *parent != &session.id)
        {
            Some(parent) => children.entry(parent).or_default().push(session),
            None => roots.push(session),
        }
    }
    let mut rows = Vec::with_capacity(sessions.len());
    let mut placed: HashSet<&SessionId> = HashSet::new();
    let mut stack: Vec<(&SessionSummary, usize, bool)> =
        roots.into_iter().rev().map(|root| (root, 0, false)).collect();
    while let Some((session, depth, hidden)) = stack.pop() {
        if !placed.insert(&session.id) {
            continue;
        }
        let kids = children.get(&session.id).map_or(&[][..], Vec::as_slice);
        let is_collapsed = collapsed.contains(&session.id);
        if !hidden {
            rows.push(Row {
                session: session.clone(),
                depth,
                children: kids.len(),
                collapsed: is_collapsed,
            });
        }
        if depth + 1 < DEPTH_CAP {
            for kid in kids.iter().rev() {
                stack.push((kid, depth + 1, hidden || is_collapsed));
            }
        }
    }
    for session in sessions {
        if !placed.contains(&session.id) {
            rows.push(Row {
                session: session.clone(),
                depth: 0,
                children: 0,
                collapsed: false,
            });
        }
    }
    rows
}

/// The sessions `wanted` picks, plus every listed ancestor of one, so a kept subagent
/// still sits under its parent. Order is unchanged.
pub fn keep(sessions: &[SessionSummary], wanted: impl Fn(&SessionSummary) -> bool) -> Vec<SessionSummary> {
    let parents: HashMap<&SessionId, &SessionId> = sessions
        .iter()
        .filter_map(|session| Some((&session.id, session.parent.as_ref()?)))
        .collect();
    let mut kept: HashSet<&SessionId> = HashSet::new();
    for session in sessions.iter().filter(|session| wanted(session)) {
        let mut next = Some(&session.id);
        while let Some(id) = next {
            if !kept.insert(id) {
                break;
            }
            next = parents.get(id).copied();
        }
    }
    sessions
        .iter()
        .filter(|session| kept.contains(&session.id))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{keep, nest};
    use crate::session::{SessionStatus, SessionSummary};
    use crate::{HarnessId, NativeSessionId, SessionId};

    fn id(native: &str) -> SessionId {
        SessionId::new(HarnessId::new("claude-code"), NativeSessionId::new(native))
    }

    fn session(native: &str, parent: Option<&str>) -> SessionSummary {
        SessionSummary {
            id: id(native),
            title: Some(native.into()),
            status: SessionStatus::Idle,
            model: None,
            cwd: None,
            updated_at_ms: 0,
            parent: parent.map(id),
            node: None,
            blocked: false,
        }
    }

    #[test]
    fn children_sit_under_their_parent_in_order() {
        let sessions = [
            session("a", None),
            session("b", None),
            session("a1", Some("a")),
            session("a1x", Some("a1")),
        ];
        let rows = nest(&sessions, &HashSet::new());
        let order: Vec<(&str, usize)> = rows
            .iter()
            .map(|row| (row.session.title.as_deref().unwrap_or(""), row.depth))
            .collect();
        assert_eq!(order, [("a", 0), ("a1", 1), ("a1x", 2), ("b", 0)]);
        assert_eq!(rows[0].children, 1);
    }

    #[test]
    fn collapsing_hides_every_descendant_and_orphans_stay_listed() {
        let sessions = [
            session("a", None),
            session("a1", Some("a")),
            session("a1x", Some("a1")),
            session("o", Some("gone")),
        ];
        let collapsed = HashSet::from([id("a")]);
        let rows = nest(&sessions, &collapsed);
        let titles: Vec<&str> = rows
            .iter()
            .map(|row| row.session.title.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(titles, ["a", "o"]);
        assert!(rows[0].collapsed);
    }

    #[test]
    fn a_parent_cycle_never_drops_a_row() {
        let sessions = [session("x", Some("y")), session("y", Some("x"))];
        let rows = nest(&sessions, &HashSet::new());
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn hiding_finished_keeps_a_finished_parent_of_a_live_subagent() {
        let mut done = session("a", None);
        done.status = SessionStatus::Ended;
        let mut failed = session("b", None);
        failed.status = SessionStatus::Failed;
        let sessions = [done, failed, session("a1", Some("a"))];
        let kept = keep(&sessions, |session| !session.status.is_finished());
        let titles: Vec<&str> = kept
            .iter()
            .map(|session| session.title.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(titles, ["a", "a1"]);
    }
}

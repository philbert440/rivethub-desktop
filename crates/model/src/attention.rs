use std::cmp::Reverse;

use crate::SessionId;
use crate::session::SessionSummary;

pub fn next_needing_you<'a>(
    sessions: &'a [SessionSummary],
    current: Option<&SessionId>,
    blocked: impl Fn(&SessionSummary) -> bool,
) -> Option<&'a SessionSummary> {
    let mut waiting: Vec<&SessionSummary> = sessions.iter().filter(|session| blocked(session)).collect();
    waiting.sort_by_key(|session| Reverse(session.updated_at_ms));
    let after = current
        .and_then(|id| waiting.iter().position(|session| &session.id == id))
        .map_or(0, |index| index + 1);
    waiting.get(after).or_else(|| waiting.first()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionStatus;
    use crate::{HarnessId, NativeSessionId};

    fn session(native: &str, updated_at_ms: i64, blocked: bool) -> SessionSummary {
        SessionSummary {
            id: SessionId::new(HarnessId::new("claude-code"), NativeSessionId::new(native)),
            title: None,
            status: SessionStatus::Active,
            model: None,
            cwd: None,
            updated_at_ms,
            parent: None,
            node: None,
            blocked,
        }
    }

    fn next<'a>(sessions: &'a [SessionSummary], current: Option<&SessionId>) -> Option<&'a str> {
        next_needing_you(sessions, current, |session| session.blocked)
            .map(|session| session.id.native().as_str())
    }

    #[test]
    fn jumps_cycle_through_waiting_sessions_newest_first() {
        let sessions = [
            session("old", 1, true),
            session("calm", 5, false),
            session("new", 9, true),
        ];
        assert_eq!(next(&sessions, None), Some("new"));
        assert_eq!(next(&sessions, Some(&sessions[2].id)), Some("old"));
        assert_eq!(next(&sessions, Some(&sessions[0].id)), Some("new"));
        assert_eq!(next(&sessions, Some(&sessions[1].id)), Some("new"));
    }

    #[test]
    fn nothing_waiting_means_no_jump() {
        let sessions = [session("calm", 5, false)];
        assert_eq!(next(&sessions, None), None);
    }
}

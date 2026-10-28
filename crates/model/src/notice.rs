use crate::SessionId;
use crate::prefs::Preferences;
use crate::session::SessionStatus;

/// Why a session is worth a desktop notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// The agent finished a reply.
    Finished,
    /// The session waits on an approval or an answer.
    NeedsYou,
}

impl NoticeKind {
    /// What a session's live state moving from (`was`, `was_blocked`) to (`now`, `blocked`) asks
    /// of the person. A session seen for the first time never counts as finished, so reconnecting
    /// doesn't replay old turns.
    pub fn raised_by(
        was: Option<SessionStatus>,
        was_blocked: bool,
        now: SessionStatus,
        blocked: bool,
    ) -> Option<Self> {
        if blocked && !was_blocked {
            Some(Self::NeedsYou)
        } else {
            (was == Some(SessionStatus::Active) && now == SessionStatus::Idle).then_some(Self::Finished)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub session: SessionId,
}

impl Notice {
    /// Whether to show this notice: the person turned its kind on, and the session isn't the one
    /// already on screen in a focused window.
    pub fn worth_showing(&self, prefs: &Preferences, focused: bool, on_screen: Option<&SessionId>) -> bool {
        prefs.wants(self.kind) && !(focused && on_screen == Some(&self.session))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HarnessId, NativeSessionId};

    #[test]
    fn a_reply_finishes_only_on_an_active_to_idle_edge() {
        use SessionStatus::{Active, Ended, Idle};
        assert_eq!(
            NoticeKind::raised_by(Some(Active), false, Idle, false),
            Some(NoticeKind::Finished)
        );
        assert_eq!(NoticeKind::raised_by(None, false, Idle, false), None);
        assert_eq!(NoticeKind::raised_by(Some(Idle), false, Idle, false), None);
        assert_eq!(NoticeKind::raised_by(Some(Active), false, Ended, false), None);
    }

    #[test]
    fn needing_you_is_raised_once_when_the_session_blocks() {
        use SessionStatus::Active;
        assert_eq!(
            NoticeKind::raised_by(Some(Active), false, Active, true),
            Some(NoticeKind::NeedsYou)
        );
        assert_eq!(NoticeKind::raised_by(Some(Active), true, Active, true), None);
    }

    #[test]
    fn the_session_on_screen_in_a_focused_window_is_not_shown() {
        let id = SessionId::new(HarnessId::new("claude-code"), NativeSessionId::new("a"));
        let notice = Notice {
            kind: NoticeKind::Finished,
            session: id.clone(),
        };
        let prefs = Preferences::default();
        assert!(!notice.worth_showing(&prefs, true, Some(&id)));
        assert!(notice.worth_showing(&prefs, false, Some(&id)));
        assert!(notice.worth_showing(&prefs, true, None));
        let quiet = Preferences {
            notify_finished: false,
            ..prefs
        };
        assert!(!notice.worth_showing(&quiet, false, None));
    }
}

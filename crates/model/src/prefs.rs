use crate::notice::NoticeKind;

/// The choices the person makes in Settings, kept between runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    pub notify_finished: bool,
    pub notify_needs_you: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            notify_finished: true,
            notify_needs_you: true,
        }
    }
}

impl Preferences {
    pub fn wants(&self, kind: NoticeKind) -> bool {
        match kind {
            NoticeKind::Finished => self.notify_finished,
            NoticeKind::NeedsYou => self.notify_needs_you,
        }
    }
}

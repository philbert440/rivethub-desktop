use crate::HarnessId;
use crate::notice::NoticeKind;

/// The choices the person makes in Settings, kept between runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    pub notify_finished: bool,
    pub notify_needs_you: bool,
    /// A reply arriving or a turn ending brings an open chat to its end, even when scrolled up.
    pub jump_to_newest: bool,
    /// What the launcher preselects; `None` leaves it to the built-in choice.
    pub last_launch: Option<LastLaunch>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            notify_finished: true,
            notify_needs_you: true,
            jump_to_newest: true,
            last_launch: None,
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

/// The last thing a new session was started with. The agent's name is kept so Settings can show
/// it without reading the roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LastLaunch {
    Agent {
        id: String,
        name: String,
    },
    /// `node` is `None` for this client's node.
    Harness {
        node: Option<String>,
        harness: HarnessId,
        model: Option<String>,
    },
}

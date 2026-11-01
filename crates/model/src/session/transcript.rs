//! The turns a session view holds from its node's transcript: the window the node follows, and
//! the pages of a long transcript read back before it.

use super::{ContextUsage, SessionView, ToolOutput, Turn, written_plan};

/// The turns of a long transcript read back before a point, and where the ones before them start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarlierPage {
    pub turns: Vec<Turn>,
    pub earlier: Option<u64>,
}

/// How much of a long transcript the view holds: the window the node follows, and the pages read
/// back before it, which sit at the front of the turns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Earlier {
    /// Where the window the node follows starts.
    window: Option<u64>,
    /// How many of the turns came from pages read back before the window.
    paged: usize,
    /// Where the turns held start, when there are turns before them to read back.
    cursor: Option<u64>,
    loading: bool,
}

impl SessionView {
    /// The transcript as the node reads it now, behind the pages read back before it while the
    /// window it follows stays where it was.
    pub(super) fn snapshot(
        &mut self,
        mut turns: Vec<Turn>,
        context: Option<ContextUsage>,
        earlier: Option<u64>,
    ) {
        self.keep_outputs(&mut turns);
        if let Some(plan) = written_plan(&turns) {
            self.plan = plan;
        }
        let mut held: Vec<Turn> = if earlier == self.earlier.window {
            self.turns.drain(..self.earlier.paged).collect()
        } else {
            self.earlier = Earlier {
                window: earlier,
                cursor: earlier,
                ..Earlier::default()
            };
            Vec::new()
        };
        held.append(&mut turns);
        self.turns = held;
        self.live = None;
        if context.is_some() {
            self.context = context;
        }
    }

    /// Where the turns held start, when there are earlier ones to read back from there.
    pub fn earlier(&self) -> Option<u64> {
        self.earlier.cursor
    }

    /// Where the turns held start: the offset a fork counts its prompts from.
    pub fn held_from(&self) -> u64 {
        self.earlier.cursor.unwrap_or(0)
    }

    pub fn loading_earlier(&self) -> bool {
        self.earlier.loading
    }

    /// Starts reading back the page before the turns held, and says where it ends; none when
    /// there is no earlier page or one is already on its way.
    pub fn begin_earlier(&mut self) -> Option<u64> {
        let cursor = self.earlier.cursor.filter(|_| !self.earlier.loading)?;
        self.earlier.loading = true;
        Some(cursor)
    }

    /// Puts the page read back before `before` in front of the turns held, unless the window it
    /// was read for has moved since.
    pub fn earlier_read(&mut self, before: u64, page: EarlierPage) {
        self.earlier.loading = false;
        if self.earlier.cursor != Some(before) {
            return;
        }
        let EarlierPage { mut turns, earlier } = page;
        self.earlier.paged += turns.len();
        self.earlier.cursor = earlier;
        turns.append(&mut self.turns);
        self.turns = turns;
    }

    pub fn earlier_failed(&mut self) {
        self.earlier.loading = false;
    }

    fn keep_outputs(&self, incoming: &mut [Turn]) {
        let mut known: std::collections::HashMap<&str, &ToolOutput> = self
            .turns
            .iter()
            .chain(self.live.as_ref())
            .flat_map(|turn| &turn.tools)
            .filter_map(|tool| Some((tool.id.as_deref()?, tool.output.as_ref()?)))
            .collect();
        for tool in incoming.iter_mut().flat_map(|turn| &mut turn.tools) {
            if tool.output.is_none()
                && let Some(output) = tool.id.as_deref().and_then(|id| known.remove(id))
            {
                tool.output = Some(output.clone());
            }
        }
    }
}

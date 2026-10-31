use std::collections::{HashMap, HashSet};

use gpui::{ListAlignment, ListState, px};
use model::markdown::Block;
use model::memory::SessionMemory;

/// How far past the window the summaries list lays out, so a scroll finds them ready.
const OVERDRAW: f32 = 800.;

/// A session's or conversation's memory with its summaries parsed, and which of them are open.
pub(super) struct Recalled {
    pub(super) memory: SessionMemory,
    pub(super) blocks: HashMap<String, Vec<Block>>,
    pub(super) expanded: HashSet<String>,
    /// The summaries as the outline shows them: each one's depth and place in `memory`.
    pub(super) order: Vec<(usize, usize)>,
    /// The page's rows, its head and then each summary; a session can hold a thousand summaries,
    /// so only the rows in view are laid out and drawn.
    pub(super) rows: ListState,
}

impl Recalled {
    /// Takes memory as it loaded, keeping open whatever was open before and where the page was.
    pub(super) fn new(memory: SessionMemory, before: Option<Self>) -> Self {
        let blocks = memory
            .summaries
            .iter()
            .map(|summary| (summary.id.clone(), model::markdown::parse(summary.text.trim())))
            .collect();
        let places: HashMap<&str, usize> = memory
            .summaries
            .iter()
            .enumerate()
            .map(|(place, summary)| (summary.id.as_str(), place))
            .collect();
        let order: Vec<(usize, usize)> = memory
            .outline()
            .into_iter()
            .filter_map(|(depth, summary)| Some((depth, *places.get(summary.id.as_str())?)))
            .collect();
        let count = order.len() + 1;
        let (expanded, rows) = match before {
            Some(before) => {
                let had = before.rows.item_count();
                if had == count {
                    before.rows.remeasure();
                } else {
                    before.rows.splice(had.min(count)..had, count.saturating_sub(had));
                    before.rows.remeasure();
                }
                (before.expanded, before.rows)
            }
            None => (
                HashSet::new(),
                ListState::new(count, ListAlignment::Top, px(OVERDRAW)),
            ),
        };
        Self {
            memory,
            blocks,
            expanded,
            order,
            rows,
        }
    }

    pub(super) fn toggle(&mut self, summary: &str) {
        if !self.expanded.remove(summary) {
            self.expanded.insert(summary.to_owned());
        }
        let row = self.order.iter().position(|(_, place)| {
            self.memory
                .summaries
                .get(*place)
                .is_some_and(|known| known.id == summary)
        });
        if let Some(row) = row {
            self.rows.remeasure_items(row + 1..row + 2);
        }
    }
}

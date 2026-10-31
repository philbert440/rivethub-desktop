use std::collections::{HashMap, HashSet};

use gpui::{ListAlignment, ListState, px};
use model::markdown::Block;
use model::memory::{SessionMemory, Summary};

/// How far past the window the summaries list lays out, so a scroll finds them ready.
const OVERDRAW: f32 = 800.;

/// A session's or conversation's memory with its summaries parsed, and which of them are open.
pub(super) struct Recalled {
    /// The memory as it loaded, each summary cut to its start where the outline cut it.
    pub(super) memory: SessionMemory,
    pub(super) blocks: HashMap<String, Vec<Block>>,
    pub(super) expanded: HashSet<String>,
    /// The summaries read whole since, by id, kept while the memory loads again.
    whole: HashMap<String, String>,
    /// The summaries as the outline shows them: each one's depth and place in `memory`.
    pub(super) order: Vec<(usize, usize)>,
    /// The page's rows, its head and then each summary; a session can hold a thousand summaries,
    /// so only the rows in view are laid out and drawn.
    pub(super) rows: ListState,
}

/// A summary's words to show: the whole when it has been read, else its start, marked as cut.
fn shown(summary: &Summary, whole: &HashMap<String, String>) -> Vec<Block> {
    match whole.get(&summary.id) {
        Some(text) => model::markdown::parse(text.trim()),
        None if summary.truncated => model::markdown::parse(&format!("{}…", summary.text.trim_end())),
        None => model::markdown::parse(summary.text.trim()),
    }
}

impl Recalled {
    /// Takes memory as it loaded, keeping open whatever was open before, what was read whole and
    /// where the page was.
    pub(super) fn new(memory: SessionMemory, before: Option<Self>) -> Self {
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
        let (expanded, whole, rows) = match before {
            Some(before) => {
                let had = before.rows.item_count();
                if had != count {
                    before.rows.splice(had.min(count)..had, count.saturating_sub(had));
                }
                before.rows.remeasure();
                (before.expanded, before.whole, before.rows)
            }
            None => (
                HashSet::new(),
                HashMap::new(),
                ListState::new(count, ListAlignment::Top, px(OVERDRAW)),
            ),
        };
        let blocks = memory
            .summaries
            .iter()
            .map(|summary| (summary.id.clone(), shown(summary, &whole)))
            .collect();
        Self {
            memory,
            blocks,
            expanded,
            whole,
            order,
            rows,
        }
    }

    /// Opens or closes a summary, answering whether opening it wants its whole text read.
    pub(super) fn toggle(&mut self, summary: &str) -> bool {
        let opened = self.expanded.insert(summary.to_owned());
        if !opened {
            self.expanded.remove(summary);
        }
        self.remeasure(summary);
        opened
            && !self.whole.contains_key(summary)
            && self
                .memory
                .summaries
                .iter()
                .any(|known| known.id == summary && known.truncated)
    }

    /// Shows a summary whole, as it was read on its own.
    pub(super) fn read_whole(&mut self, summary: Summary) {
        self.whole.insert(summary.id.clone(), summary.text);
        if let Some(known) = self.memory.summaries.iter().find(|known| known.id == summary.id) {
            self.blocks.insert(known.id.clone(), shown(known, &self.whole));
        }
        self.remeasure(&summary.id);
    }

    fn remeasure(&self, summary: &str) {
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

use std::collections::{HashMap, HashSet};

use gpui::{ListAlignment, ListOffset, ListState, px};
use model::markdown::Block;
use model::memory::{SessionMemory, Summary};

/// How far past the window the summaries list lays out, so a scroll finds them ready.
const OVERDRAW: f32 = 800.;
/// How many rows past the last one drawn have their summaries' children read ahead.
const READ_AHEAD: usize = 12;

/// A session's or conversation's memory with its summaries parsed, and which of them are open.
pub(super) struct Recalled {
    /// The memory as far as it has been read, each summary cut to its start where rh cut it.
    pub(super) memory: SessionMemory,
    pub(super) blocks: HashMap<String, Vec<Block>>,
    pub(super) expanded: HashSet<String>,
    /// The summaries read whole since, by id, kept while the memory loads again.
    whole: HashMap<String, String>,
    /// The summaries as the outline shows them: each one's depth and place in `memory`.
    pub(super) order: Vec<(usize, usize)>,
    /// The page's rows: its head, each summary, then a last row while more of the top level is
    /// left to read. A session can hold thousands of summaries, so only the rows in view are
    /// laid out and drawn.
    pub(super) rows: ListState,
    /// How far a memory read a level at a time has been read; `None` for one read whole.
    pub(super) tree: Option<Tree>,
}

/// What of a memory read a level at a time has been read, or asked for.
pub(super) struct Tree {
    /// How many summaries sit at the top level, and how many of them have been read.
    top: u64,
    read: usize,
    asking_top: bool,
    /// The summaries whose children have been asked for.
    asked: HashSet<String>,
}

/// What to read next for the rows around one being drawn.
#[derive(Default)]
pub(super) struct Wants {
    pub(super) children: Vec<String>,
    /// Where the next window of the top level starts, when it's wanted.
    pub(super) top_from: Option<usize>,
}

/// A summary's words to show: the whole when it has been read, else its start, marked as cut.
fn shown(summary: &Summary, whole: &HashMap<String, String>) -> Vec<Block> {
    match whole.get(&summary.id) {
        Some(text) => model::markdown::parse(text.trim()),
        None if summary.truncated => model::markdown::parse(&format!("{}…", summary.text.trim_end())),
        None => model::markdown::parse(summary.text.trim()),
    }
}

fn order_of(memory: &SessionMemory) -> Vec<(usize, usize)> {
    let places: HashMap<&str, usize> = memory
        .summaries
        .iter()
        .enumerate()
        .map(|(place, summary)| (summary.id.as_str(), place))
        .collect();
    memory
        .outline()
        .into_iter()
        .filter_map(|(depth, summary)| Some((depth, *places.get(summary.id.as_str())?)))
        .collect()
}

impl Recalled {
    /// Takes memory as it loaded, keeping open whatever was open before, what was read whole and
    /// where the page was. Memory that came with counts is the first window of its top level.
    pub(super) fn new(memory: SessionMemory, before: Option<Self>) -> Self {
        let order = order_of(&memory);
        let tree = memory.counts.map(|counts| Tree {
            top: counts.top,
            read: memory.summaries.len(),
            asking_top: false,
            asked: HashSet::new(),
        });
        let anchor = before.as_ref().map(Self::anchor);
        let (expanded, whole, rows) = match before {
            Some(before) => (before.expanded, before.whole, before.rows),
            None => (
                HashSet::new(),
                HashMap::new(),
                ListState::new(0, ListAlignment::Top, px(OVERDRAW)),
            ),
        };
        let blocks = memory
            .summaries
            .iter()
            .map(|summary| (summary.id.clone(), shown(summary, &whole)))
            .collect();
        let recalled = Self {
            memory,
            blocks,
            expanded,
            whole,
            order,
            rows,
            tree,
        };
        recalled.rows.reset(recalled.row_count());
        if let Some(anchor) = anchor {
            recalled.restore(anchor);
        }
        recalled
    }

    pub(super) fn more_left(&self) -> bool {
        self.tree
            .as_ref()
            .is_some_and(|tree| u64::try_from(tree.read).is_ok_and(|read| read < tree.top))
    }

    fn row_count(&self) -> usize {
        1 + self.order.len() + usize::from(self.more_left())
    }

    /// Whether this memory, as it loaded again, says what the page already shows.
    pub(super) fn shows(&self, memory: &SessionMemory) -> bool {
        if self.tree.is_none() {
            return self.memory == *memory;
        }
        self.memory.counts == memory.counts
            && self.memory.messages == memory.messages
            && memory
                .summaries
                .iter()
                .all(|summary| self.memory.summaries.iter().any(|known| known == summary))
    }

    /// Takes summaries read since: new ones join the outline under the summary they roll up
    /// into, known ones take their latest words and counts. The rows keep the one at the top.
    pub(super) fn merge(&mut self, summaries: Vec<Summary>) {
        let anchor = self.anchor();
        for summary in summaries {
            let place = self
                .memory
                .summaries
                .iter()
                .position(|known| known.id == summary.id);
            if let Some(tree) = self.tree.as_mut()
                && let Some(place) = place
                && self.memory.summaries.get(place).map(|known| known.children) != Some(summary.children)
            {
                tree.asked.remove(&summary.id);
            }
            self.blocks
                .insert(summary.id.clone(), shown(&summary, &self.whole));
            match place.and_then(|place| self.memory.summaries.get_mut(place)) {
                Some(known) => *known = summary,
                None => self.memory.summaries.push(summary),
            }
        }
        self.order = order_of(&self.memory);
        self.rows.reset(self.row_count());
        self.restore(anchor);
    }

    /// Takes the memory as it loaded again: its counts and tally, and its first window.
    pub(super) fn refresh(&mut self, memory: SessionMemory) {
        if let Some(tree) = self.tree.as_mut()
            && let Some(counts) = memory.counts
        {
            tree.top = counts.top;
        }
        self.memory.counts = memory.counts;
        self.memory.messages = memory.messages;
        self.merge(memory.summaries);
    }

    /// Takes the next window of the top level.
    pub(super) fn more_top(&mut self, memory: SessionMemory) {
        if let Some(tree) = self.tree.as_mut() {
            tree.asking_top = false;
            tree.read += memory.summaries.len();
            if let Some(counts) = memory.counts {
                tree.top = counts.top;
            }
        }
        self.merge(memory.summaries);
    }

    /// The reads the rows from this one on want: the children of summaries that have some not
    /// read yet, and the top level's next window once its last row is near.
    pub(super) fn wants(&mut self, row: usize) -> Wants {
        let mut wants = Wants::default();
        let more = self.more_left();
        let last = self.order.len() + 1;
        let Some(tree) = self.tree.as_mut() else {
            return wants;
        };
        for at in row.max(1)..row.saturating_add(READ_AHEAD).min(last + 1) {
            if at == last {
                if more && !tree.asking_top {
                    tree.asking_top = true;
                    wants.top_from = Some(tree.read);
                }
                continue;
            }
            let Some(summary) = self
                .order
                .get(at - 1)
                .and_then(|(_, place)| self.memory.summaries.get(*place))
            else {
                continue;
            };
            if summary.children.is_some_and(|children| children > 0) && tree.asked.insert(summary.id.clone())
            {
                wants.children.push(summary.id.clone());
            }
        }
        wants
    }

    /// The summary at the top of the rows, and how far into it, or the head.
    fn anchor(&self) -> (Option<String>, ListOffset) {
        let top = self.rows.logical_scroll_top();
        let id = top
            .item_ix
            .checked_sub(1)
            .and_then(|at| self.order.get(at))
            .and_then(|(_, place)| self.memory.summaries.get(*place))
            .map(|summary| summary.id.clone());
        (id, top)
    }

    fn restore(&self, (id, top): (Option<String>, ListOffset)) {
        let item_ix = match id {
            Some(id) => self
                .order
                .iter()
                .position(|(_, place)| {
                    self.memory
                        .summaries
                        .get(*place)
                        .is_some_and(|known| known.id == id)
                })
                .map_or(top.item_ix, |at| at + 1),
            None => top.item_ix.min(self.row_count().saturating_sub(1)),
        };
        self.rows.scroll_to(ListOffset {
            item_ix,
            offset_in_item: top.offset_in_item,
        });
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

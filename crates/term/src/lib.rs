mod keys;
mod links;
mod palette;

pub use keys::{Modifiers, encode_key};
pub use links::Link;
pub use palette::Rgb;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode, viewport_to_point};
use alacritty_terminal::vte::ansi::{CursorShape, Processor};
use links::find_links;
use parking_lot::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub columns: u16,
    pub rows: u16,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        usize::from(self.rows)
    }
    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }
    fn columns(&self) -> usize {
        usize::from(self.columns)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub fg: Rgb,
    pub bg: Rgb,
    pub look: Look,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectBy {
    Character,
    Word,
    Line,
}

/// A position on the visible grid; `right_half` says which half of the cell was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub row: usize,
    pub column: usize,
    pub right_half: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub lines: Vec<Vec<Span>>,
    pub cursor: Option<(usize, usize)>,
    pub size: GridSize,
    pub background: Rgb,
}

/// What the terminal raises while parsing: replies owed to the program, and whether it rang the bell.
#[derive(Clone, Default)]
struct Replies {
    bytes: Arc<Mutex<Vec<u8>>>,
    bell: Arc<AtomicBool>,
}

impl EventListener for Replies {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => self.bytes.lock().extend_from_slice(text.as_bytes()),
            Event::Bell => self.bell.store(true, Ordering::Relaxed),
            _ => {}
        }
    }
}

pub struct Screen {
    term: Term<Replies>,
    parser: Processor,
    replies: Replies,
    size: GridSize,
}

impl std::fmt::Debug for Screen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Screen")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Screen {
    pub fn new(size: GridSize) -> Self {
        let replies = Replies::default();
        let config = Config {
            scrolling_history: 10_000,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &size, replies.clone()),
            parser: Processor::new(),
            replies,
            size,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.parser.advance(&mut self.term, bytes);
        std::mem::take(&mut *self.replies.bytes.lock())
    }

    /// Whether the bell rang since the last call.
    pub fn take_bell(&self) -> bool {
        self.replies.bell.swap(false, Ordering::Relaxed)
    }

    pub fn resize(&mut self, size: GridSize) {
        if size != self.size && size.columns > 0 && size.rows > 0 {
            self.size = size;
            self.term.resize(size);
        }
    }

    pub fn size(&self) -> GridSize {
        self.size
    }

    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn select(&mut self, hit: Hit, by: SelectBy) {
        let kind = match by {
            SelectBy::Character => SelectionType::Simple,
            SelectBy::Word => SelectionType::Semantic,
            SelectBy::Line => SelectionType::Lines,
        };
        let (point, side) = self.locate(hit);
        self.term.selection = Some(Selection::new(kind, point, side));
    }

    pub fn extend_selection(&mut self, hit: Hit) {
        let (point, side) = self.locate(hit);
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    pub fn selected_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|text| !text.is_empty())
    }

    fn locate(&self, hit: Hit) -> (Point, Side) {
        let row = hit.row.min(usize::from(self.size.rows).saturating_sub(1));
        let column = hit.column.min(usize::from(self.size.columns).saturating_sub(1));
        let point = viewport_to_point(self.term.grid().display_offset(), Point::new(row, Column(column)));
        (point, if hit.right_half { Side::Right } else { Side::Left })
    }

    /// The web address under a hit, if there is one. Its columns count characters, as snapshot
    /// spans do, so a wide character's second cell maps back onto the character.
    pub fn link_at(&self, hit: Hit) -> Option<Link> {
        if hit.row >= usize::from(self.size.rows) {
            return None;
        }
        let grid = self.term.grid();
        let offset = i32::try_from(grid.display_offset()).ok()?;
        let line = Line(i32::try_from(hit.row).ok()? - offset);
        let mut text = String::new();
        let mut chars = 0usize;
        let mut under = None;
        for (column, cell) in grid[line].into_iter().enumerate() {
            if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                text.push(cell.c);
                chars += 1;
            }
            if column == hit.column {
                under = chars.checked_sub(1);
            }
        }
        let under = under?;
        find_links(&text)
            .into_iter()
            .find(|link| link.columns.contains(&under))
    }

    pub fn background(&self) -> Rgb {
        palette::background(self.term.colors())
    }

    pub fn app_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn bracketed_paste(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    pub fn snapshot(&self) -> Snapshot {
        let content = self.term.renderable_content();
        let colors = content.colors;
        let offset = content.display_offset;
        let selection = content.selection;
        let mut lines: Vec<Vec<Span>> = vec![Vec::new(); usize::from(self.size.rows)];
        for indexed in content.display_iter {
            let cell = indexed.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let row = usize::try_from(indexed.point.line.0 + i32::try_from(offset).unwrap_or(i32::MAX)).ok();
            let Some(line) = row.and_then(|row| lines.get_mut(row)) else {
                continue;
            };
            let bold = cell.flags.contains(Flags::BOLD);
            let mut fg = palette::resolve(cell.fg, colors, bold);
            let mut bg = palette::resolve(cell.bg, colors, false);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(Flags::DIM) {
                fg = fg.dimmed();
            }
            let look = Look {
                bold,
                italic: cell.flags.contains(Flags::ITALIC),
                underline: cell.flags.contains(Flags::UNDERLINE),
            };
            let selected = selection.is_some_and(|range| range.contains(indexed.point));
            match line.last_mut() {
                Some(span)
                    if span.fg == fg && span.bg == bg && span.look == look && span.selected == selected =>
                {
                    span.text.push(cell.c);
                }
                _ => line.push(Span {
                    text: cell.c.to_string(),
                    fg,
                    bg,
                    look,
                    selected,
                }),
            }
        }
        let cursor = (offset == 0 && content.cursor.shape != CursorShape::Hidden)
            .then(|| {
                usize::try_from(content.cursor.point.line.0)
                    .ok()
                    .map(|row| (row, content.cursor.point.column.0))
            })
            .flatten();
        Snapshot {
            lines,
            cursor,
            size: self.size,
            background: palette::background(colors),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GridSize, Hit, Screen, SelectBy};

    #[test]
    fn colored_text_lands_in_one_span_per_style() {
        let mut screen = Screen::new(GridSize { columns: 20, rows: 3 });
        screen.feed(b"plain \x1b[31mred\x1b[0m");
        let snapshot = screen.snapshot();
        let first = &snapshot.lines[0];
        assert_eq!(first[0].text, "plain ");
        assert_eq!(first[1].text, "red");
        assert_ne!(first[0].fg, first[1].fg);
        assert_eq!(snapshot.cursor, Some((0, 9)));
    }

    #[test]
    fn a_drag_selects_text_across_lines() {
        let mut screen = Screen::new(GridSize { columns: 20, rows: 3 });
        screen.feed(b"first line\r\nsecond line");
        let at = |row, column| Hit {
            row,
            column,
            right_half: false,
        };
        screen.select(at(0, 6), SelectBy::Character);
        screen.extend_selection(at(1, 6));
        assert_eq!(screen.selected_text().as_deref(), Some("line\nsecond"));
        assert!(
            screen.snapshot().lines[0]
                .iter()
                .any(|span| span.selected && span.text.starts_with("line"))
        );
        screen.select(at(1, 8), SelectBy::Word);
        assert_eq!(screen.selected_text().as_deref(), Some("line"));
    }

    #[test]
    fn a_link_is_found_under_a_cell_past_wide_characters() {
        let mut screen = Screen::new(GridSize { columns: 30, rows: 2 });
        screen.feed("日本 https://a.dev ok".as_bytes());
        let at = |column| Hit {
            row: 0,
            column,
            right_half: false,
        };
        let link = screen.link_at(at(5)).map(|link| (link.url, link.columns));
        assert_eq!(link, Some(("https://a.dev".to_owned(), 3..16)));
        assert_eq!(screen.link_at(at(4)), None);
    }

    #[test]
    fn a_cursor_position_query_produces_a_reply_for_the_pty() {
        let mut screen = Screen::new(GridSize { columns: 20, rows: 3 });
        let reply = screen.feed(b"ab\x1b[6n");
        assert_eq!(reply, b"\x1b[1;3R");
    }
}

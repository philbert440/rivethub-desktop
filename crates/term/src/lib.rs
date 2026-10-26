mod keys;
mod palette;

pub use keys::{Modifiers, encode_key};
pub use palette::Rgb;

use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, Processor};
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
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub lines: Vec<Vec<Span>>,
    pub cursor: Option<(usize, usize)>,
    pub size: GridSize,
    pub background: Rgb,
}

#[derive(Clone, Default)]
struct Replies(Arc<Mutex<Vec<u8>>>);

impl EventListener for Replies {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            self.0.lock().extend_from_slice(text.as_bytes());
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
        std::mem::take(&mut *self.replies.0.lock())
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
            let italic = cell.flags.contains(Flags::ITALIC);
            let underline = cell.flags.contains(Flags::UNDERLINE);
            match line.last_mut() {
                Some(span)
                    if span.fg == fg
                        && span.bg == bg
                        && span.bold == bold
                        && span.italic == italic
                        && span.underline == underline =>
                {
                    span.text.push(cell.c);
                }
                _ => line.push(Span {
                    text: cell.c.to_string(),
                    fg,
                    bg,
                    bold,
                    italic,
                    underline,
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
    use super::{GridSize, Screen};

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
    fn a_cursor_position_query_produces_a_reply_for_the_pty() {
        let mut screen = Screen::new(GridSize { columns: 20, rows: 3 });
        let reply = screen.feed(b"ab\x1b[6n");
        assert_eq!(reply, b"\x1b[1;3R");
    }
}

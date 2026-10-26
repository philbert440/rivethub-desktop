use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub marks: Marks,
    pub code: bool,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Paragraph(Vec<Run>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Quote(Vec<Block>),
    Code {
        language: Option<String>,
        text: String,
    },
    Table {
        head: Vec<Vec<Run>>,
        rows: Vec<Vec<Vec<Run>>>,
    },
    Rule,
}

#[derive(Default)]
struct Style {
    bold: usize,
    italic: usize,
    strike: usize,
    link: Vec<String>,
}

enum Frame {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Item(Vec<Block>),
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Paragraph(Vec<Run>),
    Code {
        language: Option<String>,
        text: String,
    },
    Table {
        head: Vec<Vec<Run>>,
        rows: Vec<Vec<Vec<Run>>>,
        in_head: bool,
    },
    Row(Vec<Vec<Run>>),
    Cell(Vec<Run>),
}

struct Builder {
    stack: Vec<Frame>,
    style: Style,
}

impl Builder {
    fn runs(&mut self) -> Option<&mut Vec<Run>> {
        match self.stack.last_mut()? {
            Frame::Heading { runs, .. } | Frame::Paragraph(runs) | Frame::Cell(runs) => Some(runs),
            _ => None,
        }
    }

    fn push_text(&mut self, text: &str, code: bool) {
        if let Some(Frame::Code { text: body, .. }) = self.stack.last_mut() {
            body.push_str(text);
            return;
        }
        if self.runs().is_none() {
            self.stack.push(Frame::Paragraph(Vec::new()));
        }
        let run = Run {
            text: text.to_owned(),
            marks: Marks {
                bold: self.style.bold > 0,
                italic: self.style.italic > 0,
                strike: self.style.strike > 0,
            },
            code,
            link: self.style.link.last().cloned(),
        };
        if let Some(runs) = self.runs() {
            match runs.last_mut() {
                Some(last) if !code && !last.code && last.marks == run.marks && last.link == run.link => {
                    last.text.push_str(&run.text);
                }
                _ => runs.push(run),
            }
        }
    }

    fn close_loose_paragraph(&mut self) {
        if matches!(self.stack.last(), Some(Frame::Paragraph(_))) && self.stack.len() > 1 {
            self.close();
        }
    }

    fn emit(&mut self, block: Block) {
        if let Some(Frame::Root(blocks) | Frame::Quote(blocks) | Frame::Item(blocks)) = self.stack.last_mut()
        {
            blocks.push(block);
        }
    }

    fn close(&mut self) {
        let Some(frame) = self.stack.pop() else { return };
        let block = match frame {
            Frame::Root(blocks) => {
                self.stack.push(Frame::Root(blocks));
                return;
            }
            Frame::Quote(blocks) => Block::Quote(blocks),
            Frame::List { start, items } => Block::List { start, items },
            Frame::Item(blocks) => {
                if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
                    items.push(blocks);
                }
                return;
            }
            Frame::Heading { level, runs } => Block::Heading { level, runs },
            Frame::Paragraph(runs) => Block::Paragraph(runs),
            Frame::Code { language, text } => Block::Code { language, text },
            Frame::Table { head, rows, .. } => Block::Table { head, rows },
            Frame::Row(cells) => {
                if let Some(Frame::Table { head, rows, in_head }) = self.stack.last_mut() {
                    if *in_head { *head = cells } else { rows.push(cells) }
                }
                return;
            }
            Frame::Cell(runs) => {
                match self.stack.last_mut() {
                    Some(Frame::Row(cells)) => cells.push(runs),
                    Some(Frame::Table {
                        head, in_head: true, ..
                    }) => head.push(runs),
                    _ => {}
                }
                return;
            }
        };
        self.emit(block);
    }
}

fn level(heading: HeadingLevel) -> u8 {
    match heading {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

pub fn parse(source: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut builder = Builder {
        stack: vec![Frame::Root(Vec::new())],
        style: Style::default(),
    };
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => builder.stack.push(Frame::Paragraph(Vec::new())),
                Tag::Heading { level: heading, .. } => {
                    builder.stack.push(Frame::Heading {
                        level: level(heading),
                        runs: Vec::new(),
                    });
                }
                Tag::BlockQuote(_) => builder.stack.push(Frame::Quote(Vec::new())),
                Tag::CodeBlock(kind) => {
                    builder.close_loose_paragraph();
                    let language = match kind {
                        CodeBlockKind::Fenced(info) => info
                            .split_whitespace()
                            .next()
                            .filter(|lang| !lang.is_empty())
                            .map(str::to_owned),
                        CodeBlockKind::Indented => None,
                    };
                    builder.stack.push(Frame::Code {
                        language,
                        text: String::new(),
                    });
                }
                Tag::List(start) => {
                    builder.close_loose_paragraph();
                    builder.stack.push(Frame::List {
                        start,
                        items: Vec::new(),
                    });
                }
                Tag::Item => builder.stack.push(Frame::Item(Vec::new())),
                Tag::Table(_) => builder.stack.push(Frame::Table {
                    head: Vec::new(),
                    rows: Vec::new(),
                    in_head: false,
                }),
                Tag::TableHead => {
                    if let Some(Frame::Table { in_head, .. }) = builder.stack.last_mut() {
                        *in_head = true;
                    }
                }
                Tag::TableRow => builder.stack.push(Frame::Row(Vec::new())),
                Tag::TableCell => builder.stack.push(Frame::Cell(Vec::new())),
                Tag::Emphasis => builder.style.italic += 1,
                Tag::Strong => builder.style.bold += 1,
                Tag::Strikethrough => builder.style.strike += 1,
                Tag::Link { dest_url, .. } => builder.style.link.push(dest_url.into_string()),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::BlockQuote(_)
                | TagEnd::CodeBlock
                | TagEnd::List(_)
                | TagEnd::Table
                | TagEnd::TableRow
                | TagEnd::TableCell => builder.close(),
                TagEnd::Item => {
                    builder.close_loose_paragraph();
                    builder.close();
                }
                TagEnd::TableHead => {
                    if let Some(Frame::Table { in_head, .. }) = builder.stack.last_mut() {
                        *in_head = false;
                    }
                }
                TagEnd::Emphasis => builder.style.italic = builder.style.italic.saturating_sub(1),
                TagEnd::Strong => builder.style.bold = builder.style.bold.saturating_sub(1),
                TagEnd::Strikethrough => builder.style.strike = builder.style.strike.saturating_sub(1),
                TagEnd::Link => {
                    builder.style.link.pop();
                }
                _ => {}
            },
            Event::Text(text) => builder.push_text(&text, false),
            Event::Code(text) => builder.push_text(&text, true),
            Event::SoftBreak => builder.push_text(" ", false),
            Event::HardBreak => builder.push_text("\n", false),
            Event::Rule => builder.emit(Block::Rule),
            Event::TaskListMarker(done) => builder.push_text(if done { "☑ " } else { "☐ " }, false),
            _ => {}
        }
    }
    while builder.stack.len() > 1 {
        builder.close();
    }
    match builder.stack.pop() {
        Some(Frame::Root(blocks)) => blocks,
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, parse};

    #[test]
    fn headings_paragraphs_and_code_become_blocks() {
        let blocks = parse("# Title\n\nSome **bold** and `code`.\n\n```rust\nfn main() {}\n```\n");
        assert!(matches!(&blocks[0], Block::Heading { level: 1, runs } if runs[0].text == "Title"));
        let Block::Paragraph(runs) = &blocks[1] else {
            unreachable!("second block is a paragraph")
        };
        assert!(runs.iter().any(|run| run.marks.bold && run.text == "bold"));
        assert!(runs.iter().any(|run| run.code && run.text == "code"));
        assert!(
            matches!(&blocks[2], Block::Code { language: Some(lang), text } if lang == "rust" && text == "fn main() {}\n")
        );
    }

    #[test]
    fn tight_lists_and_tables_keep_their_structure() {
        let blocks = parse("1. one\n2. two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
        let Block::List {
            start: Some(1),
            items,
        } = &blocks[0]
        else {
            unreachable!("first block is a list")
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[1][0], Block::Paragraph(runs) if runs[0].text == "two"));
        let Block::Table { head, rows } = &blocks[1] else {
            unreachable!("second block is a table")
        };
        assert_eq!(head.len(), 2);
        assert_eq!(rows[0][1][0].text, "2");
    }

    #[test]
    fn an_unclosed_fence_while_streaming_still_renders_as_code() {
        let blocks = parse("Here:\n\n```py\nprint(1)");
        assert!(matches!(&blocks[1], Block::Code { text, .. } if text.starts_with("print(1)")));
    }
}

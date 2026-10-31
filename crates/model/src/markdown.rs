use std::ops::Range;

mod stream;

pub use stream::Parsed;

use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd, TextMergeStream,
};

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
    /// The wiki page a `[[slug]]` link names, in a page parsed with `parse_page`.
    pub page: Option<String>,
}

impl Run {
    /// Where clicking the run goes: only a web or mail address, since a relative or local target
    /// would resolve against wherever the app happens to run rather than the session's folder.
    pub fn url(&self) -> Option<&str> {
        self.link.as_deref().filter(|target| {
            ["https://", "http://", "mailto:"].iter().any(|scheme| {
                target
                    .get(..scheme.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
            })
        })
    }
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
    link: Vec<Target>,
}

/// Where a link goes: an address, or a wiki page by its slug.
#[derive(Clone)]
enum Target {
    Url(String),
    Page(String),
}

fn target(kind: LinkType, url: String) -> Target {
    if matches!(kind, LinkType::WikiLink { .. }) {
        Target::Page(url)
    } else {
        Target::Url(url)
    }
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
        let target = self.style.link.last().cloned();
        if code || target.is_some() {
            self.push_run(text, code, target);
            return;
        }
        let mut from = 0;
        for url in bare_links(text) {
            self.push_run(&text[from..url.start], false, None);
            self.push_run(
                &text[url.clone()],
                false,
                Some(Target::Url(text[url.clone()].to_owned())),
            );
            from = url.end;
        }
        self.push_run(&text[from..], false, None);
    }

    fn push_run(&mut self, text: &str, code: bool, target: Option<Target>) {
        if text.is_empty() {
            return;
        }
        let (link, page) = match target {
            Some(Target::Url(url)) => (Some(url), None),
            Some(Target::Page(slug)) => (None, Some(slug)),
            None => (None, None),
        };
        let run = Run {
            text: text.to_owned(),
            marks: Marks {
                bold: self.style.bold > 0,
                italic: self.style.italic > 0,
                strike: self.style.strike > 0,
            },
            code,
            link,
            page,
        };
        if let Some(runs) = self.runs() {
            match runs.last_mut() {
                Some(last)
                    if !code
                        && !last.code
                        && last.marks == run.marks
                        && last.link == run.link
                        && last.page == run.page =>
                {
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

/// The web addresses written out in plain text, which a reply means as links even without link syntax.
/// An address ends at whitespace, and the punctuation that closes the sentence around it isn't part of it.
fn bare_links(text: &str) -> Vec<Range<usize>> {
    let mut links = Vec::new();
    let mut from = 0;
    while let Some(found) = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| text[from..].find(scheme).map(|at| (from + at, scheme.len())))
        .min()
    {
        let (start, scheme) = found;
        let rest = &text[start..];
        let mut end = start
            + rest
                .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`'))
                .unwrap_or(rest.len());
        loop {
            let url = &text[start..end];
            let trimmed = url.trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '"']);
            let unbalanced =
                trimmed.ends_with(')') && trimmed.matches(')').count() > trimmed.matches('(').count();
            let trimmed = if unbalanced {
                &trimmed[..trimmed.len() - 1]
            } else {
                trimmed
            };
            if trimmed.len() == url.len() {
                break;
            }
            end = start + trimmed.len();
        }
        let joined = text[..start]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric);
        if end > start + scheme && !joined {
            links.push(start..end);
        }
        from = end.max(start + scheme);
    }
    links
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

/// The words of a markdown snippet without its markup: no list markers, emphasis, code ticks or
/// link targets, blocks joined by spaces. For one-line previews of a reply.
pub fn plain(source: &str) -> String {
    let mut words = Vec::new();
    for block in parse(source) {
        flatten(&block, &mut words);
    }
    words.join(" ")
}

fn flatten(block: &Block, words: &mut Vec<String>) {
    let runs = |runs: &[Run]| {
        runs.iter()
            .map(|run| run.text.as_str())
            .collect::<String>()
            .replace('\n', " ")
    };
    match block {
        Block::Heading { runs: line, .. } | Block::Paragraph(line) => words.push(runs(line)),
        Block::List { items, .. } => items.iter().flatten().for_each(|inner| flatten(inner, words)),
        Block::Quote(inner) => inner.iter().for_each(|inner| flatten(inner, words)),
        Block::Code { text, .. } => words.push(text.trim().to_owned()),
        Block::Table { head, rows } => {
            words.extend(head.iter().chain(rows.iter().flatten()).map(|cell| runs(cell)));
        }
        Block::Rule => {}
    }
}

const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS);

pub fn parse(source: &str) -> Vec<Block> {
    parse_with(source, OPTIONS)
}

/// A wiki page's markdown, where `[[slug]]` and `[[slug|text]]` link to other pages.
pub fn parse_page(source: &str) -> Vec<Block> {
    parse_with(source, OPTIONS.union(Options::ENABLE_WIKILINKS))
}

/// The pages a wiki page's `[[links]]` name, each once, in the order they first appear.
pub fn page_links(blocks: &[Block]) -> Vec<String> {
    let mut slugs = Vec::new();
    let mut note = |runs: &[Run]| {
        for slug in runs.iter().filter_map(|run| run.page.as_ref()) {
            if !slugs.contains(slug) {
                slugs.push(slug.clone());
            }
        }
    };
    let mut pending: Vec<&Block> = blocks.iter().rev().collect();
    while let Some(block) = pending.pop() {
        match block {
            Block::Heading { runs, .. } | Block::Paragraph(runs) => note(runs),
            Block::List { items, .. } => pending.extend(items.iter().flatten().rev()),
            Block::Quote(inner) => pending.extend(inner.iter().rev()),
            Block::Table { head, rows } => head
                .iter()
                .chain(rows.iter().flatten())
                .for_each(|cell| note(cell)),
            Block::Code { .. } | Block::Rule => {}
        }
    }
    slugs
}

fn parse_with(source: &str, options: Options) -> Vec<Block> {
    let mut builder = Builder {
        stack: vec![Frame::Root(Vec::new())],
        style: Style::default(),
    };
    for event in TextMergeStream::new(Parser::new_ext(source, options)) {
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
                Tag::Link {
                    link_type, dest_url, ..
                } => builder.style.link.push(target(link_type, dest_url.into_string())),
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
            Event::SoftBreak | Event::HardBreak => builder.push_text("\n", false),
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
    #[test]
    fn only_a_page_parse_reads_wiki_links() {
        let source = "- [[ct117]]\n- [[datahub-backup|the backup move]]";
        assert_eq!(
            super::page_links(&super::parse_page(source)),
            ["ct117", "datahub-backup"]
        );
        assert_eq!(super::page_links(&super::parse(source)), Vec::<String>::new());
    }

    #[test]
    fn plain_keeps_the_words_and_drops_the_markup() {
        assert_eq!(super::plain("- **whoami:** `rivet`"), "whoami: rivet");
        assert_eq!(
            super::plain("See [the PR](https://example.com) _now_."),
            "See the PR now."
        );
    }

    use super::{Block, bare_links, parse};

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
    fn bare_addresses_become_links_without_the_punctuation_around_them() {
        let text = "See https://example.com/a_b. Or (http://x.org/path) and myhttps://no, then https://";
        let found: Vec<&str> = bare_links(text).into_iter().map(|range| &text[range]).collect();
        assert_eq!(found, ["https://example.com/a_b", "http://x.org/path"]);
        let wiki = "https://en.wikipedia.org/wiki/Rust_(programming_language)";
        let found: Vec<&str> = bare_links(wiki).into_iter().map(|range| &wiki[range]).collect();
        assert_eq!(found, [wiki]);
    }

    #[test]
    fn links_keep_their_target_and_code_stays_plain() {
        let blocks = parse(
            "Read [the docs](https://docs.rs) or https://crates.io, not `https://code` or [here](src/main.rs).",
        );
        let Block::Paragraph(runs) = &blocks[0] else {
            unreachable!("one paragraph")
        };
        let links: Vec<(&str, Option<&str>)> = runs
            .iter()
            .filter(|run| run.link.is_some() || run.code)
            .map(|run| (run.text.as_str(), run.url()))
            .collect();
        assert_eq!(
            links,
            [
                ("the docs", Some("https://docs.rs")),
                ("https://crates.io", Some("https://crates.io")),
                ("https://code", None),
                ("here", None),
            ]
        );
    }

    #[test]
    fn a_single_newline_breaks_the_line_as_written() {
        let Block::Paragraph(runs) = &parse("Line one\nLine two")[0] else {
            unreachable!("one paragraph")
        };
        assert_eq!(
            runs.iter().map(|run| run.text.as_str()).collect::<String>(),
            "Line one\nLine two"
        );
        assert_eq!(super::plain("Line one\nLine two"), "Line one Line two");
    }

    #[test]
    fn an_unclosed_fence_while_streaming_still_renders_as_code() {
        let blocks = parse("Here:\n\n```py\nprint(1)");
        assert!(matches!(&blocks[1], Block::Code { text, .. } if text.starts_with("print(1)")));
    }
}

use super::{Block, parse};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parsed {
    pub blocks: Vec<Block>,
    stable_len: usize,
    stable_blocks: usize,
}

fn opens_block_alone(line: &str) -> bool {
    let first = line.chars().next();
    let numbered = line
        .split_once(". ")
        .is_some_and(|(head, _)| !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()));
    !matches!(first, None | Some(' ' | '\t' | '-' | '*' | '+' | '>' | '|')) && !numbered
}

fn stable_boundary(text: &str, from: usize) -> usize {
    let mut fenced = false;
    let mut boundary = from;
    let mut offset = 0;
    let mut previous_blank = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let fence = trimmed.trim_start().starts_with("```") || trimmed.trim_start().starts_with("~~~");
        if !fenced && previous_blank && offset > from && opens_block_alone(trimmed) {
            boundary = offset;
        }
        if fence {
            fenced = !fenced;
        }
        previous_blank = !fenced && trimmed.trim().is_empty();
        offset += line.len();
    }
    boundary
}

impl Parsed {
    pub fn new(text: &str) -> Self {
        Self::default().advance("", text)
    }

    #[must_use]
    pub fn advance(&self, old_text: &str, text: &str) -> Self {
        let reuse = old_text
            .get(..self.stable_len)
            .is_some_and(|stable| text.starts_with(stable));
        let (kept_len, kept_blocks) = if reuse {
            (self.stable_len, self.stable_blocks)
        } else {
            (0, 0)
        };
        let boundary = stable_boundary(text, kept_len);
        let mut blocks: Vec<Block> = self.blocks[..kept_blocks].to_vec();
        let settled = parse(&text[kept_len..boundary]);
        let stable_blocks = kept_blocks + settled.len();
        blocks.extend(settled);
        blocks.extend(parse(&text[boundary..]));
        Self {
            blocks,
            stable_len: boundary,
            stable_blocks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn streaming_parses_match_a_full_parse_at_every_step() {
        let text = "# Plan\n\nFirst paragraph with **bold**.\n\n- one\n- two\n\n- three\n\n```rust\nfn main() {\n\n    println!(\"x\");\n}\n```\n\nAfter the code.\n\n1. first\n2. second\n\n> quoted\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nDone.";
        let mut parsed = super::Parsed::default();
        let mut previous = "";
        for end in (0..=text.len()).filter(|end| text.is_char_boundary(*end)) {
            let now = &text[..end];
            parsed = parsed.advance(previous, now);
            assert_eq!(parsed.blocks, parse(now), "after {end} bytes");
            previous = now;
        }
    }
}

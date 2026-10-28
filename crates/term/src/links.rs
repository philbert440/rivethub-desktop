use std::ops::Range;

/// A web address in one row of terminal text, by character position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub columns: Range<usize>,
    pub url: String,
}

const SCHEMES: [&str; 2] = ["https://", "http://"];

/// The http(s) addresses in a row. An address runs to the next space, quote or angle bracket;
/// trailing sentence punctuation and closing brackets it never opened are left off, so prose like
/// `(see https://example.com/a).` links just the address.
pub fn find_links(row: &str) -> Vec<Link> {
    let chars: Vec<char> = row.chars().collect();
    let mut links = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let rest = &chars[at..];
        let Some(scheme) = SCHEMES
            .iter()
            .find(|scheme| scheme.chars().eq(rest.iter().copied().take(scheme.len())))
        else {
            at += 1;
            continue;
        };
        let body = at + scheme.len();
        let mut end = body + chars[body..].iter().take_while(|&&c| in_url(c)).count();
        while end > body && trailing(&chars[at..end]) {
            end -= 1;
        }
        if end > body {
            links.push(Link {
                columns: at..end,
                url: chars[at..end].iter().collect(),
            });
            at = end;
        } else {
            at = body;
        }
    }
    links
}

fn in_url(c: char) -> bool {
    !c.is_whitespace() && !c.is_control() && !matches!(c, '<' | '>' | '"' | '\'' | '`')
}

/// Whether the address's last character belongs to the surrounding text instead.
fn trailing(url: &[char]) -> bool {
    let unopened = |open: char, close: char| {
        url.iter().filter(|&&c| c == close).count() > url.iter().filter(|&&c| c == open).count()
    };
    match url.last() {
        Some('.' | ',' | ';' | ':' | '!' | '?') => true,
        Some(')') => unopened('(', ')'),
        Some(']') => unopened('[', ']'),
        Some('}') => unopened('{', '}'),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::find_links;

    fn urls(row: &str) -> Vec<String> {
        find_links(row).into_iter().map(|link| link.url).collect()
    }

    #[test]
    fn addresses_stop_at_spaces_quotes_and_surrounding_punctuation() {
        assert_eq!(
            urls("see https://example.com/a?b=1, or http://x.io."),
            ["https://example.com/a?b=1", "http://x.io"]
        );
        assert_eq!(
            urls("(docs: https://en.wikipedia.org/wiki/Rust_(language))."),
            ["https://en.wikipedia.org/wiki/Rust_(language)"]
        );
        assert_eq!(urls("href=\"https://a.dev/x\">"), ["https://a.dev/x"]);
        assert_eq!(
            urls("https:// and ftp://host and plain text"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let links = find_links("→ é https://a.dev");
        assert_eq!(links[0].columns, 4..17);
    }
}

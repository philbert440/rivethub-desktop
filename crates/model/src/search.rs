use crate::SessionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub session: SessionId,
    /// The session's title, as rh names it beside the hit.
    pub title: Option<String>,
    pub text: String,
    /// When the matched message was sent or said.
    pub at_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snippet {
    pub before: String,
    pub matched: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMatch {
    pub session: SessionId,
    pub title: Option<String>,
    pub count: usize,
    pub snippet: Snippet,
    /// When the message the snippet shows was sent or said, which opening the session goes to.
    pub snippet_ms: i64,
    pub latest_ms: i64,
}

const CONTEXT: usize = 48;

/// Every word of the query turns up in at least one of the fields, ignoring case.
pub fn mentions<'a>(fields: impl IntoIterator<Item = &'a str>, query: &str) -> bool {
    let fields: Vec<String> = fields.into_iter().map(str::to_lowercase).collect();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| fields.iter().any(|field| field.contains(word)))
}

fn find(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let lower = haystack.to_lowercase();
    if lower.len() != haystack.len() {
        return haystack.find(needle);
    }
    lower.find(&needle.to_lowercase())
}

pub fn snippet(text: &str, query: &str) -> Snippet {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut terms: Vec<&str> = query.split_whitespace().collect();
    terms.sort_by_key(|term| std::cmp::Reverse(term.len()));
    let found = find(&flat, query.trim())
        .map(|at| (at, query.trim().len()))
        .or_else(|| {
            terms
                .iter()
                .find_map(|term| find(&flat, term).map(|at| (at, term.len())))
        });
    let Some((at, len)) = found else {
        let head: String = flat.chars().take(CONTEXT * 2).collect();
        return Snippet {
            before: head,
            ..Snippet::default()
        };
    };
    let start = flat[..at]
        .char_indices()
        .rev()
        .nth(CONTEXT)
        .map_or(0, |(index, _)| index);
    let end = flat[at + len..]
        .char_indices()
        .nth(CONTEXT)
        .map_or(flat.len(), |(index, _)| at + len + index);
    let before = if start > 0 {
        format!("…{}", &flat[start..at])
    } else {
        flat[..at].to_owned()
    };
    let after = if end < flat.len() {
        format!("{}…", &flat[at + len..end])
    } else {
        flat[at + len..].to_owned()
    };
    Snippet {
        before,
        matched: flat[at..at + len].to_owned(),
        after,
    }
}

pub fn group(hits: Vec<Hit>, query: &str) -> Vec<SessionMatch> {
    let mut matches: Vec<SessionMatch> = Vec::new();
    for hit in hits {
        if let Some(existing) = matches.iter_mut().find(|found| found.session == hit.session) {
            existing.count += 1;
            existing.latest_ms = existing.latest_ms.max(hit.at_ms);
            if existing.title.is_none() {
                existing.title = hit.title;
            }
            continue;
        }
        matches.push(SessionMatch {
            snippet: snippet(&hit.text, query),
            session: hit.session,
            title: hit.title,
            count: 1,
            snippet_ms: hit.at_ms,
            latest_ms: hit.at_ms,
        });
    }
    matches.sort_by(|a, b| b.count.cmp(&a.count).then(b.latest_ms.cmp(&a.latest_ms)));
    matches
}

#[cfg(test)]
mod tests {
    use super::{Hit, group, mentions, snippet};
    use crate::{HarnessId, NativeSessionId, SessionId};

    fn session(native: &str) -> SessionId {
        SessionId::new(HarnessId::new("claude-code"), NativeSessionId::new(native))
    }

    #[test]
    fn a_snippet_centres_on_the_match_case_insensitively() {
        let found = snippet(
            "rh cert renew refuses a cert past its Renewal Deadline, exactly when it must renew",
            "renewal deadline",
        );
        assert_eq!(found.matched, "Renewal Deadline");
        assert!(found.before.ends_with("past its "));
        assert!(found.after.starts_with(", exactly"));
    }

    #[test]
    fn hits_group_by_session_with_the_busiest_first() {
        let hit = |native: &str, at_ms| Hit {
            session: session(native),
            title: None,
            text: "the deadline".into(),
            at_ms,
        };
        let grouped = group(vec![hit("a", 1), hit("b", 5), hit("b", 2)], "deadline");
        assert_eq!(grouped[0].session, session("b"));
        assert_eq!(grouped[0].count, 2);
        assert_eq!(grouped[0].latest_ms, 5);
        assert_eq!(grouped[1].count, 1);
    }

    #[test]
    fn every_word_must_turn_up_in_some_field() {
        assert!(mentions(["Cert renewal", "ct115"], "renew CT115"));
        assert!(!mentions(["Cert renewal", "ct115"], "renew phildesk"));
        assert!(mentions(["anything"], "  "));
    }
}

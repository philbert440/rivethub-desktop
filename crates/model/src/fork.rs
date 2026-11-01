//! Where a fork came from: the chat it was forked from and how much of that chat it carries.

use crate::SessionId;
use crate::chat::Exchange;

/// A fork's source, kept by the app that made it: rh hands the fork the source's conversation as
/// context and keeps no record of where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkOrigin {
    pub source: SessionId,
    pub node: Option<String>,
    pub title: Option<String>,
    /// The fork carries the source's conversation up to the reply to this many prompts.
    pub prompts: u32,
    /// Where in the source's transcript those prompts are counted from; none for a fork
    /// remembered before that was kept, counted in the source's latest part.
    pub from: Option<u64>,
}

/// The source's exchanges the fork carries: everything up to and including the reply to its
/// `prompts`th prompt.
pub fn carried(source: Vec<Exchange>, prompts: u32) -> Vec<Exchange> {
    let mut asked = 0;
    let mut kept = Vec::new();
    for exchange in source {
        if exchange.prompt.is_some() {
            if asked == prompts {
                break;
            }
            asked += 1;
        }
        kept.push(exchange);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::carried;
    use crate::chat::Exchange;

    fn asked(prompt: Option<&str>) -> Exchange {
        Exchange::new(prompt.map(str::to_owned), Vec::new(), None)
    }

    #[test]
    fn a_fork_carries_the_exchanges_up_to_its_prompt() {
        let source = vec![
            asked(None),
            asked(Some("one")),
            asked(Some("two")),
            asked(Some("three")),
        ];
        let kept: Vec<_> = carried(source, 2)
            .into_iter()
            .map(|exchange| exchange.prompt)
            .collect();
        assert_eq!(kept, vec![None, Some("one".into()), Some("two".into())]);
    }
}

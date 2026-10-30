use std::collections::HashMap;

use crate::markdown::Block;
use crate::{SessionId, TopicId};

/// A wiki page. A root topic owns one, and its subtopics are sections on it; an entity owns its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub slug: String,
    pub title: String,
    pub topic: Option<TopicId>,
    pub tags: Vec<String>,
    pub updated_ms: i64,
    pub markdown: String,
    /// What the page says is true now, on its own, for editing.
    pub current_state: String,
    pub article: Option<String>,
    pub citations: Vec<Citation>,
}

/// A summary that cites a page, and the conversation it summarizes, as far as rh knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub summary: String,
    pub at_ms: Option<i64>,
    pub conversation: Option<String>,
    /// The harness session the conversation is, when it is one.
    pub session: Option<SessionId>,
    pub title: Option<String>,
    pub channel: Option<String>,
    /// Where the conversation is filed.
    pub topic: Option<TopicId>,
}

/// One conversation that cites a page, with how often and how lately: a row of "Mentioned in".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub conversation: String,
    pub session: Option<SessionId>,
    pub title: Option<String>,
    pub channel: Option<String>,
    pub topic: Option<TopicId>,
    pub summaries: usize,
    pub latest_ms: Option<i64>,
}

/// The conversations that cite a page, each once, the most recently cited first. A citation whose
/// summary rh no longer has names no conversation and is left out.
pub fn mentioned_in(citations: &[Citation]) -> Vec<Mention> {
    let mut by_conversation: HashMap<&str, Mention> = HashMap::new();
    for citation in citations {
        let Some(conversation) = citation.conversation.as_deref() else {
            continue;
        };
        let mention = by_conversation.entry(conversation).or_insert_with(|| Mention {
            conversation: conversation.to_owned(),
            session: citation.session.clone(),
            title: citation.title.clone(),
            channel: citation.channel.clone(),
            topic: citation.topic.clone(),
            summaries: 0,
            latest_ms: None,
        });
        mention.summaries += 1;
        mention.latest_ms = mention.latest_ms.max(citation.at_ms);
    }
    let mut mentions: Vec<Mention> = by_conversation.into_values().collect();
    mentions.sort_by(|left, right| {
        right
            .latest_ms
            .cmp(&left.latest_ms)
            .then_with(|| left.conversation.cmp(&right.conversation))
    });
    mentions
}

/// The topics the mentioning conversations are filed in, the one with the most of them first.
pub fn mentioning_topics(mentions: &[Mention]) -> Vec<(TopicId, usize)> {
    let mut counted: HashMap<&TopicId, usize> = HashMap::new();
    for topic in mentions.iter().filter_map(|mention| mention.topic.as_ref()) {
        *counted.entry(topic).or_default() += 1;
    }
    let mut topics: Vec<(TopicId, usize)> = counted
        .into_iter()
        .map(|(topic, count)| (topic.clone(), count))
        .collect();
    topics.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    topics
}

/// An entity a topic's sessions refer to, as the topic's Mentions line names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityMention {
    pub entity: TopicId,
    pub name: String,
}

/// What a topic or entity owns in the wiki: its page, if the wiki has written one, and the
/// entities a work topic's sessions mention.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Owned {
    pub page: Option<Page>,
    pub mentions: Vec<EntityMention>,
}

/// What a person changes on a page: its current state, its article, or both.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageEdit {
    pub current_state: Option<String>,
    pub article: Option<String>,
}

/// A page as the wiki index lists it, and the topic or entity that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub slug: String,
    pub title: String,
    pub updated_ms: i64,
    pub owner: Option<TopicId>,
}

/// Where a subtopic's section starts among a page's blocks. rh writes each subtopic as a `##`
/// section headed with the subtopic's name, after the page's own `##` sections, so the last such
/// heading wins; a section folded in from a merged page sits deeper.
pub fn section(blocks: &[Block], name: &str) -> Option<usize> {
    let name = name.trim();
    let headed = |block: &Block| match block {
        Block::Heading { level, runs } => {
            let text: String = runs.iter().map(|run| run.text.as_str()).collect();
            text.trim().eq_ignore_ascii_case(name).then_some(*level)
        }
        _ => None,
    };
    let levels: Vec<Option<u8>> = blocks.iter().map(headed).collect();
    levels
        .iter()
        .rposition(|level| *level == Some(2))
        .or_else(|| levels.iter().position(Option::is_some))
}

fn without_front_matter(markdown: &str) -> &str {
    let Some(rest) = markdown.strip_prefix("---\n") else {
        return markdown;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end() == "---" {
            return &rest[offset..];
        }
    }
    markdown
}

fn heading_level(line: &str) -> Option<usize> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    (1..=6)
        .contains(&hashes)
        .then_some(hashes)
        .filter(|_| line[hashes..].starts_with(' '))
}

/// The page without the `## Decisions` section rh renders from the topic's decisions, for a view that
/// shows those decisions itself where they can be changed.
pub fn without_decisions(markdown: &str) -> String {
    let mut fenced = false;
    let mut skipping = false;
    let mut out = String::with_capacity(markdown.len());
    for line in markdown.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        let level = if fenced { None } else { heading_level(line) };
        if let Some(level) = level.filter(|level| *level <= 2) {
            skipping = level == 2 && line[2..].trim().eq_ignore_ascii_case("Decisions");
        }
        if !skipping {
            out.push_str(line);
        }
    }
    out
}

/// The page as a reader sees it: no front matter, no title heading (the page shows its title),
/// and no headings with nothing under them, which rh leaves on pages it has not filled yet.
pub fn readable(markdown: &str) -> String {
    let lines: Vec<&str> = without_front_matter(markdown).lines().collect();
    let mut fenced = false;
    let levels: Vec<Option<usize>> = lines
        .iter()
        .map(|line| {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                return None;
            }
            if fenced { None } else { heading_level(line) }
        })
        .collect();
    let mut keep = vec![true; lines.len()];
    if let Some(first) = lines.iter().position(|line| !line.trim().is_empty())
        && levels[first] == Some(1)
    {
        keep[first] = false;
    }
    let mut next_kept: Option<(usize, usize)> = None;
    for index in (0..lines.len()).rev() {
        let Some(level) = levels[index] else {
            continue;
        };
        if !keep[index] {
            continue;
        }
        let end = next_kept.map_or(lines.len(), |(at, _)| at);
        let has_text = (index + 1..end).any(|at| levels[at].is_none() && !lines[at].trim().is_empty());
        let deeper = next_kept.is_some_and(|(_, below)| below > level);
        if has_text || deeper {
            next_kept = Some((index, level));
        } else {
            keep[index] = false;
        }
    }
    let mut out = String::new();
    let mut blank = true;
    for (line, kept) in lines.iter().zip(keep) {
        if !kept {
            continue;
        }
        let empty = line.trim().is_empty();
        if empty && blank {
            continue;
        }
        out.push_str(line);
        out.push('\n');
        blank = empty;
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::{Citation, mentioned_in, mentioning_topics, readable, section, without_decisions};
    use crate::TopicId;
    use crate::markdown::parse;

    fn cited(summary: &str, conversation: Option<&str>, at_ms: i64, topic: Option<&str>) -> Citation {
        Citation {
            summary: summary.to_owned(),
            at_ms: Some(at_ms),
            conversation: conversation.map(str::to_owned),
            session: None,
            title: None,
            channel: None,
            topic: topic.map(|topic| TopicId::new(topic.to_owned())),
        }
    }

    #[test]
    fn mentions_count_each_conversation_once_and_the_latest_first() {
        let citations = [
            cited("s1", Some("c1"), 10, Some("mesh")),
            cited("s2", Some("c2"), 30, Some("desk")),
            cited("s3", Some("c1"), 40, Some("mesh")),
            cited("s4", None, 50, None),
            cited("s5", Some("c3"), 20, Some("mesh")),
        ];
        let mentions = mentioned_in(&citations);
        let rows: Vec<(&str, usize, Option<i64>)> = mentions
            .iter()
            .map(|mention| {
                (
                    mention.conversation.as_str(),
                    mention.summaries,
                    mention.latest_ms,
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![("c1", 2, Some(40)), ("c2", 1, Some(30)), ("c3", 1, Some(20))]
        );
        assert_eq!(
            mentioning_topics(&mentions),
            vec![
                (TopicId::new("mesh".to_owned()), 2),
                (TopicId::new("desk".to_owned()), 1)
            ]
        );
    }

    #[test]
    fn a_page_reads_without_front_matter_title_or_empty_sections() {
        let page = "---\ntitle: Tools\nsections:\n  - heading: tmux\n---\n\n# Tools\n\n## Summary\n\n\n\n## History\n\n## tmux\n\nScopes differ.\n\n### Session\n\n`new-session -e`\n\n```sh\n# not a heading\n```\n";
        assert_eq!(
            readable(page),
            "## tmux\n\nScopes differ.\n\n### Session\n\n`new-session -e`\n\n```sh\n# not a heading\n```"
        );
    }

    #[test]
    fn the_decisions_section_goes_with_its_sub_topic_groups_and_nothing_else() {
        let page = "# Mesh\n\n## Decisions\n\n- Use step-ca (phil, 2026-10-27)\n\n### Certificates\n\n- Renew weekly\n\n## Summary\n\nThe mesh.\n";
        assert_eq!(without_decisions(page), "# Mesh\n\n## Summary\n\nThe mesh.\n");
    }

    #[test]
    fn a_heading_stays_while_a_filled_subsection_sits_under_it() {
        assert_eq!(
            readable("## Hosts\n\n### gerty\n\nThe incus host.\n\n### pve3\n"),
            "## Hosts\n\n### gerty\n\nThe incus host."
        );
    }

    #[test]
    fn a_subtopic_lands_on_its_own_section_heading() {
        let blocks = parse(&readable(
            "# Tools\n\n## Summary\n\nThe tools.\n\n### Writer spool\n\nAn aside.\n\n## tmux\n\nScopes.\n\n## Summary\n\nA subtopic.\n\n## Writer Spool\n\nFlushes on exit.\n",
        ));
        assert_eq!(section(&blocks, "writer spool"), Some(8));
        assert_eq!(section(&blocks, "summary"), Some(6));
        assert_eq!(section(&blocks, "tmux"), Some(4));
        assert_eq!(section(&blocks, "missing"), None);
    }
}

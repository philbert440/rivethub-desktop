use std::collections::HashSet;

use crate::TopicId;

/// A wiki page. A root topic owns one, and its subtopics are sections on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub slug: String,
    pub title: String,
    pub topic: Option<TopicId>,
    pub tags: Vec<String>,
    pub updated_ms: i64,
    pub markdown: String,
}

/// A page as the wiki index lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub slug: String,
    pub title: String,
    pub updated_ms: i64,
    pub excerpt: String,
}

/// The listed pages no topic owns.
pub fn loose<'a>(listings: &'a [Listing], owned: &[Page]) -> Vec<&'a Listing> {
    let owned: HashSet<&str> = owned.iter().map(|page| page.slug.as_str()).collect();
    listings
        .iter()
        .filter(|listing| !owned.contains(listing.slug.as_str()))
        .collect()
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
    use super::readable;

    #[test]
    fn a_page_reads_without_front_matter_title_or_empty_sections() {
        let page = "---\ntitle: Tools\nsections:\n  - heading: tmux\n---\n\n# Tools\n\n## Summary\n\n\n\n## History\n\n## tmux\n\nScopes differ.\n\n### Session\n\n`new-session -e`\n\n```sh\n# not a heading\n```\n";
        assert_eq!(
            readable(page),
            "## tmux\n\nScopes differ.\n\n### Session\n\n`new-session -e`\n\n```sh\n# not a heading\n```"
        );
    }

    #[test]
    fn a_heading_stays_while_a_filled_subsection_sits_under_it() {
        assert_eq!(
            readable("## Hosts\n\n### gerty\n\nThe incus host.\n\n### pve3\n"),
            "## Hosts\n\n### gerty\n\nThe incus host."
        );
    }
}

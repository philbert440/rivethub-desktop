use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::memory::Stats;
use model::tidy::{Upkeep, Worker};

use super::MemoryPage;
use super::tidy::COLUMN;
use crate::text::{ago_phrase, now_ms};
use crate::theme::Theme;

/// A count grouped by thousands, such as "999,695".
fn grouped(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

impl MemoryPage {
    pub(super) fn load_stats(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.memory_stats().await;
            let upkeep = hub.upkeep().await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok(stats) => this.stats = Some(stats),
                    Err(err) => this.failed(&err, cx),
                }
                match upkeep {
                    Ok(upkeep) => this.upkeep = Some(upkeep),
                    Err(err) if err.is_missing_route() => this.upkeep = None,
                    Err(err) => this.failed(&err, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// How much memory holds, then what needs a person.
    pub(super) fn stats(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("memory-stats")
            .size_full()
            .overflow_y_scroll()
            .py_8()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN))
                    .px_8()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_xl().text_color(Theme::ink()).child("Memory"))
                    .children(
                        self.problem
                            .clone()
                            .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
                    )
                    .child(self.stats.map_or_else(
                        || {
                            div()
                                .text_color(Theme::ink_dim())
                                .child("Loading…")
                                .into_any_element()
                        },
                        Self::memory_tiles,
                    ))
                    .children(self.upkeep.as_ref().map(Self::upkeep_tiles))
                    .child(div().h(px(12.)))
                    .child(self.tidy_section(cx))
                    .when_some(self.flagged_section(cx), |column, flagged| {
                        column.child(div().h(px(12.))).child(flagged)
                    }),
            )
            .into_any_element()
    }

    fn memory_tiles(stats: Stats) -> AnyElement {
        let waiting = match (stats.embed_queue, stats.failed_embeddings) {
            (0, 0) => "nothing waiting".to_owned(),
            (queued, 0) => format!("{} waiting", grouped(queued)),
            (0, failed) => format!("{} failed", grouped(failed)),
            (queued, failed) => format!("{} waiting · {} failed", grouped(queued), grouped(failed)),
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(Self::tile(
                        "Conversations",
                        grouped(stats.conversations),
                        String::new(),
                    ))
                    .child(Self::tile(
                        "Messages",
                        grouped(stats.messages),
                        format!("{} tool calls", grouped(stats.tool_calls)),
                    ))
                    .child(Self::tile("Summaries", grouped(stats.summaries), String::new())),
            )
            .child(div().flex().gap_3().child(Self::tile(
                "Searchable by meaning",
                format!(
                    "{} of {} messages",
                    grouped(stats.embedded_messages),
                    grouped(stats.messages)
                ),
                waiting,
            )))
            .into_any_element()
    }

    /// How filing, the wiki and the workers are keeping up.
    fn upkeep_tiles(upkeep: &Upkeep) -> AnyElement {
        let filing = upkeep.filing;
        let filed = filing.categorizer + filing.you;
        let wiki = &upkeep.wiki;
        let entities: u64 = wiki.entity_types.iter().map(|kind| kind.entities).sum();
        let mut types: Vec<_> = wiki
            .entity_types
            .iter()
            .filter(|kind| kind.entities > 0)
            .collect();
        types.sort_by(|left, right| {
            right
                .entities
                .cmp(&left.entities)
                .then_with(|| left.name.cmp(&right.name))
        });
        let by_type = types
            .iter()
            .take(4)
            .map(|kind| format!("{} {}", grouped(kind.entities), kind.name))
            .collect::<Vec<_>>()
            .join(" · ");
        let workers = &upkeep.workers;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().pt_3().text_color(Theme::ink()).child("Filing"))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(Self::tile(
                        "Filed",
                        grouped(filed),
                        format!(
                            "{} by the categorizer · {} by you",
                            grouped(filing.categorizer),
                            grouped(filing.you)
                        ),
                    ))
                    .child(Self::tile(
                        "Not filed yet",
                        grouped(filing.uncategorized),
                        format!("{} left out, such as test runs", grouped(filing.left_out)),
                    ))
                    .child(Self::tile(
                        "Corrections",
                        grouped(filing.corrections_last_week),
                        "in the last week".to_owned(),
                    )),
            )
            .child(div().pt_3().text_color(Theme::ink()).child("Wiki"))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(Self::tile(
                        "Topic pages",
                        grouped(wiki.topic_pages),
                        String::new(),
                    ))
                    .child(Self::tile("Entities", grouped(entities), by_type))
                    .child(Self::tile(
                        "Pages without a topic",
                        grouped(wiki.loose_pages),
                        String::new(),
                    )),
            )
            .child(div().pt_3().text_color(Theme::ink()).child("Workers"))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(Self::worker_tile("Embedder", workers.embedder))
                    .child(Self::worker_tile("Summarizer", workers.summarizer))
                    .child(Self::worker_tile("Categorizer", workers.categorizer))
                    .child(Self::worker_tile("Wiki builder", workers.wiki_builder)),
            )
            .into_any_element()
    }

    fn worker_tile(label: &'static str, worker: Worker) -> impl IntoElement {
        let last = worker
            .last_finished_ms
            .map_or_else(|| "never ran".to_owned(), |at| ago_phrase(now_ms(), at));
        let failing = match worker.failing {
            0 => "nothing failing".to_owned(),
            1 => "1 job failing".to_owned(),
            count => format!("{} jobs failing", grouped(count)),
        };
        Self::tile(label, last, failing)
    }
}

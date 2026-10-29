use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::memory::Stats;

use super::MemoryPage;
use super::tidy::COLUMN;
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
            this.update(cx, |this, cx| match loaded {
                Ok(stats) => {
                    this.stats = Some(stats);
                    cx.notify();
                }
                Err(err) => this.failed(&err, cx),
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
            .justify_center()
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
                    .child(div().h(px(12.)))
                    .child(self.tidy_section(cx)),
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
}

use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::memory::Verdict;
use model::tidy::{Batch, Kind, Proposal, Proposer};
use model::topics::Topic;

use super::MemoryPage;
use super::proposals::{Decided, counted, verdict_button};
use crate::brand;
use crate::text::{ago_phrase, now_ms};
use crate::theme::Theme;

/// An opened kind of change shows this many of its changes until asked for all of them.
const SHOWN: usize = 12;
const INDENT: f32 = 22.;

/// A kind of change as a heading, and one and many of it in a sentence.
fn kind_words(kind: Kind) -> (&'static str, &'static str, &'static str) {
    match kind {
        Kind::NewTopic => ("New topics", "new topic", "new topics"),
        Kind::NewType => ("New types", "new type", "new types"),
        Kind::FileInto => ("Sessions filed into topics", "filing", "filings"),
        Kind::Move => ("Moves", "move", "moves"),
        Kind::Rename => ("Renames", "rename", "renames"),
        Kind::Merge => ("Merges", "merge", "merges"),
        Kind::Close => ("Closes", "close", "closes"),
        Kind::Delete => ("Deletions", "deletion", "deletions"),
        Kind::MergePage => ("Wiki page merges", "page merge", "page merges"),
        Kind::RemovePage => ("Wiki page removals", "page removal", "page removals"),
    }
}

/// What a verdict button decides: one proposal, or every pending proposal of a batch.
impl MemoryPage {
    pub(super) fn batch(
        &self,
        index: usize,
        batch: &Batch,
        existing: &[Topic],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let kinds = batch.by_kind(existing);
        let changes = batch.proposals.len();
        let from = match batch.by {
            Proposer::Backfill => "From the backfill",
            Proposer::Categorizer => "From the categorizer",
        };
        let mut summary = vec![counted(changes, "change", "changes")];
        let sessions = batch.sessions();
        if sessions > 0 {
            summary.push(format!("{sessions} sessions to file"));
        }
        summary.push(ago_phrase(now_ms(), batch.at_ms));
        let confirming = self
            .review
            .confirming
            .as_ref()
            .filter(|(id, _)| *id == batch.id)
            .map(|(_, verdict)| *verdict);
        div()
            .id(("batch", index))
            .p_4()
            .rounded(px(18.))
            .bg(Theme::panel())
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_color(Theme::ink()).child(from))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(Theme::ink_dim())
                                    .child(summary.join(" · ")),
                            ),
                    )
                    .children(
                        confirming
                            .is_none()
                            .then(|| Self::batch_buttons(index, &batch.id, cx)),
                    ),
            )
            .children(confirming.map(|verdict| Self::confirm_strip(index, batch, &kinds, verdict, cx)))
            .children(
                kinds
                    .iter()
                    .enumerate()
                    .flat_map(|(at, (kind, list))| self.kind_group(batch, at, *kind, list, cx)),
            )
            .into_any_element()
    }

    /// Approve all and Reject all, each asking to be confirmed before it decides anything.
    fn batch_buttons(index: usize, batch: &str, cx: &mut Context<Self>) -> gpui::Div {
        let (approve, reject) = (batch.to_owned(), batch.to_owned());
        div()
            .flex()
            .gap_3()
            .child(
                verdict_button(("batch-reject", index), "Reject all…", Verdict::Reject).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.review.confirming = Some((reject.clone(), Verdict::Reject));
                        cx.notify();
                    }),
                ),
            )
            .child(
                verdict_button(("batch-approve", index), "Approve all…", Verdict::Accept).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.review.confirming = Some((approve.clone(), Verdict::Accept));
                        cx.notify();
                    }),
                ),
            )
    }

    /// What deciding the whole batch would do, named by count and kind, with Cancel and the
    /// button that does it.
    fn confirm_strip(
        index: usize,
        batch: &Batch,
        kinds: &[(Kind, Vec<&Proposal>)],
        verdict: Verdict,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let count = batch.proposals.len();
        let listed: Vec<String> = kinds
            .iter()
            .map(|(kind, list)| {
                let (_, one, many) = kind_words(*kind);
                counted(list.len(), one, many)
            })
            .collect();
        let (verb, warn) = match verdict {
            Verdict::Accept => ("Approve", Theme::accent()),
            Verdict::Reject => ("Reject", Theme::warn()),
        };
        let decided = batch.id.clone();
        div()
            .px_4()
            .py_3()
            .rounded(px(14.))
            .border_1()
            .border_color(warn)
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(Theme::ink())
                    .child(format!(
                        "{verb} all {}: {}?",
                        counted(count, "change", "changes"),
                        listed.join(", ")
                    )),
            )
            .child(
                verdict_button(("batch-cancel", index), "Cancel", Verdict::Reject).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.review.confirming = None;
                        cx.notify();
                    },
                )),
            )
            .child(
                verdict_button(("batch-confirm", index), format!("{verb} {count}"), verdict).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.decide_proposals(Decided::Batch(decided.clone()), verdict, cx);
                    }),
                ),
            )
            .into_any_element()
    }

    /// One kind of change in a batch: a line with its count that opens to its changes, the first
    /// few until all are asked for.
    fn kind_group(
        &self,
        batch: &Batch,
        at: usize,
        kind: Kind,
        list: &[&Proposal],
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let key = (batch.id.clone(), kind);
        let open = self.review.opened.contains(&key);
        let (heading, _, _) = kind_words(kind);
        let filed: u64 = list
            .iter()
            .filter(|proposal| proposal.change.members().is_some())
            .map(|proposal| proposal.sessions)
            .sum();
        let toggled = key.clone();
        let mut out = vec![
            div()
                .id(("kind", at))
                .min_h(px(36.))
                .px_2()
                .rounded(px(10.))
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(|line| line.bg(Theme::card()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !this.review.opened.remove(&toggled) {
                        this.review.opened.insert(toggled.clone());
                    }
                    cx.notify();
                }))
                .child(brand::icon(
                    brand::symbol(Some(if open { "expand_more" } else { "chevron_right" })),
                    18.,
                    Theme::ink_dim(),
                ))
                .child(div().text_color(Theme::ink()).child(heading))
                .child(
                    div()
                        .text_sm()
                        .text_color(Theme::ink_dim())
                        .child(list.len().to_string()),
                )
                .when(filed > 0, |line| {
                    line.child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(format!("· {filed} sessions")),
                    )
                })
                .into_any_element(),
        ];
        if !open {
            return out;
        }
        let all = self.review.all.contains(&key);
        let shown = if all { list.len() } else { list.len().min(SHOWN) };
        for proposal in list.iter().take(shown) {
            out.push(
                div()
                    .ml(px(INDENT))
                    .child(self.proposed(batch, false, proposal, None, cx))
                    .into_any_element(),
            );
            if self.review.members.contains_key(&proposal.id) {
                out.push(
                    div()
                        .ml(px(INDENT * 2.))
                        .child(self.members(proposal, cx))
                        .into_any_element(),
                );
            }
        }
        if list.len() > SHOWN {
            out.push(
                div()
                    .id(("kind-all", at))
                    .ml(px(INDENT))
                    .px_2()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(Theme::accent())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if !this.review.all.remove(&key) {
                            this.review.all.insert(key.clone());
                        }
                        cx.notify();
                    }))
                    .child(if all {
                        "Show fewer".to_owned()
                    } else {
                        format!("Show all {}", list.len())
                    })
                    .into_any_element(),
            );
        }
        out
    }
}

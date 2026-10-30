use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::TopicId;
use model::memory::Verdict;
use model::tidy::{Batch, Change, Proposal, Proposer};

use super::{MemoryEvent, MemoryPage};
use crate::text::{ago_phrase, now_ms};
use crate::theme::Theme;

/// A batch shows this many of its changes until asked for all of them.
const SHOWN: usize = 12;
const INDENT: f32 = 22.;
const SAMPLES: usize = 3;

/// What a verdict button decides: one proposal, or every pending proposal of a batch.
#[derive(Debug, Clone)]
enum Decided {
    One(String),
    Batch(String),
}

pub(super) fn verdict_button(
    id: (&'static str, usize),
    label: &'static str,
    verdict: Verdict,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(30.))
        .px_4()
        .rounded(px(15.))
        .flex()
        .items_center()
        .cursor_pointer()
        .text_sm()
        .map(|button| match verdict {
            Verdict::Accept => button
                .bg(Theme::accent())
                .text_color(Theme::on_accent())
                .hover(|button| button.bg(Theme::on_selected())),
            Verdict::Reject => button
                .border_1()
                .border_color(Theme::line())
                .text_color(Theme::ink_dim())
                .hover(|button| button.text_color(Theme::ink())),
        })
        .child(label)
}

impl MemoryPage {
    /// What a topic, entity or type id is called: a topic or type this node knows, else a new one
    /// the batch would create.
    fn named(&self, batch: &Batch, id: &TopicId) -> String {
        let known = self
            .listed
            .topics
            .iter()
            .chain(self.catalog.iter().flat_map(|catalog| catalog.types.iter()))
            .chain(
                self.catalog
                    .iter()
                    .flat_map(|catalog| catalog.entities.iter().map(|entity| &entity.topic)),
            )
            .find(|topic| &topic.id == id)
            .map(|topic| topic.name.clone());
        known
            .or_else(|| {
                batch
                    .proposals
                    .iter()
                    .find_map(|proposal| match &proposal.change {
                        Change::CreateTopic { topic, name, .. } | Change::AddType { topic, name, .. }
                            if topic == id =>
                        {
                            Some(name.clone())
                        }
                        _ => None,
                    })
            })
            .unwrap_or_else(|| "an unknown topic".to_owned())
    }

    /// The change in words, such as "New topic Rail under Desktop App".
    fn described(&self, batch: &Batch, change: &Change, nested: bool) -> String {
        let name = |id: &TopicId| self.named(batch, id);
        let under = |parent: &Option<TopicId>| match parent {
            Some(parent) if !nested => format!(" under {}", name(parent)),
            Some(_) | None => String::new(),
        };
        match change {
            Change::Merge { source, into } => format!("Merge {} into {}", name(source), name(into)),
            Change::Move {
                topic,
                parent: Some(parent),
            } => format!("Move {} under {}", name(topic), name(parent)),
            Change::Move { topic, parent: None } => format!("Move {} to the top level", name(topic)),
            Change::Rename { topic, name: to } => format!("Rename {} to “{to}”", name(topic)),
            Change::Close { topic } => format!("Close {}", name(topic)),
            Change::Delete { topic } => format!("Delete {}", name(topic)),
            Change::CreateTopic {
                name: new, parent, ..
            } => format!("New topic {new}{}", under(parent)),
            Change::AddType {
                name: new, parent, ..
            } => format!("New type {new}{}", under(parent)),
        }
    }

    fn decide_proposals(&mut self, decided: Decided, verdict: Verdict, cx: &mut Context<Self>) {
        self.problem = None;
        if let Some(tidy) = self.tidy.as_mut() {
            match &decided {
                Decided::One(id) => {
                    for batch in &mut tidy.batches {
                        batch.proposals.retain(|proposal| &proposal.id != id);
                    }
                }
                Decided::Batch(id) => tidy.batches.retain(|batch| &batch.id != id),
            }
            tidy.batches.retain(|batch| !batch.proposals.is_empty());
        }
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let outcome = match decided {
                Decided::One(id) => hub.decide_proposal(id, verdict).await.map(|()| Vec::new()),
                Decided::Batch(id) => hub.decide_batch(id, verdict).await,
            };
            this.update(cx, |this, cx| {
                match outcome {
                    Ok(failed) if failed.is_empty() => {}
                    Ok(failed) => {
                        this.problem = Some(format!(
                            "{} of them stayed pending: {}",
                            failed.len(),
                            failed.join("; ")
                        ));
                    }
                    Err(err) => this.problem = Some(format!("deciding it failed: {err}")),
                }
                if verdict == Verdict::Accept {
                    cx.emit(MemoryEvent::Refiled);
                    this.load_catalog(cx);
                }
                this.load_tidy(cx);
                this.load_stats(cx);
            })
            .ok();
        })
        .detach();
    }

    /// The tree changes waiting, each batch with its proposed tree and a way to decide all of it.
    pub(super) fn batches(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(tidy) = &self.tidy else {
            return Vec::new();
        };
        tidy.batches
            .iter()
            .enumerate()
            .map(|(index, batch)| self.batch(index, batch, cx))
            .collect()
    }

    fn batch(&self, index: usize, batch: &Batch, cx: &mut Context<Self>) -> AnyElement {
        let tree = batch.tree();
        let total = tree.len();
        let all = self.open_batches.contains(&batch.id);
        let shown = if all { total } else { total.min(SHOWN) };
        let from = match batch.by {
            Proposer::Backfill => "From the backfill",
            Proposer::Categorizer => "From the categorizer",
        };
        let mut summary = vec![format!(
            "{total} {}",
            if total == 1 { "change" } else { "changes" }
        )];
        let sessions = batch.sessions();
        if sessions > 0 {
            summary.push(format!("{sessions} sessions to file"));
        }
        summary.push(ago_phrase(now_ms(), batch.at_ms));
        let (approve, reject) = (batch.id.clone(), batch.id.clone());
        let toggle = (total > SHOWN).then(|| {
            let id = batch.id.clone();
            div()
                .id(("batch-all", index))
                .pt_1()
                .cursor_pointer()
                .text_xs()
                .text_color(Theme::accent())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !this.open_batches.remove(&id) {
                        this.open_batches.insert(id.clone());
                    }
                    cx.notify();
                }))
                .child(if all {
                    "Show fewer".to_owned()
                } else {
                    format!("Show all {total}")
                })
        });
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
                    .child(
                        verdict_button(("batch-reject", index), "Reject all", Verdict::Reject).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.decide_proposals(Decided::Batch(reject.clone()), Verdict::Reject, cx);
                            }),
                        ),
                    )
                    .child(
                        verdict_button(("batch-approve", index), "Approve all", Verdict::Accept).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.decide_proposals(Decided::Batch(approve.clone()), Verdict::Accept, cx);
                            }),
                        ),
                    ),
            )
            .children(
                tree.into_iter()
                    .take(shown)
                    .map(|(depth, proposal)| self.proposed(batch, depth, proposal, cx)),
            )
            .children(toggle)
            .into_any_element()
    }

    fn proposed(
        &self,
        batch: &Batch,
        depth: usize,
        proposal: &Proposal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = batch
            .proposals
            .iter()
            .position(|known| known.id == proposal.id)
            .unwrap_or_default();
        let samples: Vec<&str> = proposal
            .samples
            .iter()
            .take(SAMPLES)
            .map(String::as_str)
            .collect();
        let (approve, reject) = (proposal.id.clone(), proposal.id.clone());
        let indent = px(INDENT * f32::from(u8::try_from(depth.min(4)).unwrap_or(4)));
        div()
            .id(("proposed", row))
            .ml(indent)
            .px_4()
            .py_2()
            .rounded(px(14.))
            .bg(Theme::card())
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_color(Theme::ink()).child(self.described(
                        batch,
                        &proposal.change,
                        depth > 0,
                    )))
                    .when(!proposal.reason.trim().is_empty(), |text| {
                        text.child(
                            div()
                                .text_xs()
                                .text_color(Theme::ink_dim())
                                .child(proposal.reason.clone()),
                        )
                    })
                    .when(!samples.is_empty(), |text| {
                        text.child(
                            div()
                                .text_xs()
                                .text_color(Theme::ink_dim())
                                .truncate()
                                .child(format!("e.g. {}", samples.join(" · "))),
                        )
                    }),
            )
            .when(proposal.sessions > 0, |line| {
                line.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child(format!(
                            "{} {}",
                            proposal.sessions,
                            if proposal.sessions == 1 {
                                "session"
                            } else {
                                "sessions"
                            }
                        )),
                )
            })
            .child(
                verdict_button(("proposal-reject", row), "Reject", Verdict::Reject).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        this.decide_proposals(Decided::One(reject.clone()), Verdict::Reject, cx);
                    },
                )),
            )
            .child(
                verdict_button(("proposal-approve", row), "Approve", Verdict::Accept).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        this.decide_proposals(Decided::One(approve.clone()), Verdict::Accept, cx);
                    },
                )),
            )
            .into_any_element()
    }
}

use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::memory::{TagChange, Verdict};

use super::MemoryPage;
use crate::theme::Theme;

pub(super) const COLUMN: f32 = 900.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Tag(usize),
    Term(usize),
}

impl MemoryPage {
    pub(super) fn load_tidy(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.tidy().await;
            this.update(cx, |this, cx| match loaded {
                Ok(tidy) => {
                    this.tidy = Some(tidy);
                    cx.notify();
                }
                Err(err) => this.failed(&err, cx),
            })
            .ok();
        })
        .detach();
    }

    fn decide(&mut self, item: Item, verdict: Verdict, cx: &mut Context<Self>) {
        let Some(tidy) = self.tidy.as_mut() else {
            return;
        };
        let hub = self.hub.clone();
        let sent = match item {
            Item::Tag(index) if index < tidy.tags.len() => {
                let tag = tidy.tags.remove(index);
                cx.spawn(async move |this, cx| {
                    let decided = hub.decide_tag(tag.id, verdict).await;
                    this.update(cx, |this, cx| this.decided(decided, cx)).ok();
                })
            }
            Item::Term(index) if index < tidy.terms.len() => {
                let term = tidy.terms.remove(index);
                cx.spawn(async move |this, cx| {
                    let decided = hub.decide_term(term, verdict).await;
                    this.update(cx, |this, cx| this.decided(decided, cx)).ok();
                })
            }
            Item::Tag(_) | Item::Term(_) => return,
        };
        sent.detach();
        cx.notify();
    }

    fn decided(&mut self, decided: Result<(), gateway::GatewayError>, cx: &mut Context<Self>) {
        if let Err(err) = decided {
            self.failed(&err, cx);
            self.load_tidy(cx);
        }
    }

    fn proposal(item: Item, what: String, why: String, cx: &mut Context<Self>) -> AnyElement {
        let (id, index) = match item {
            Item::Tag(index) => ("tag", index),
            Item::Term(index) => ("term", index),
        };
        let button = |label: &'static str, verdict: Verdict| {
            div()
                .id((
                    if verdict == Verdict::Accept {
                        "accept"
                    } else {
                        "reject"
                    },
                    index,
                ))
                .h(px(32.))
                .px_4()
                .rounded(px(16.))
                .flex()
                .items_center()
                .cursor_pointer()
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
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.decide(item, verdict, cx)))
                .child(label)
        };
        div()
            .id((id, index))
            .px_4()
            .py_3()
            .rounded(px(16.))
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
                    .gap_1()
                    .child(div().text_color(Theme::ink()).child(what))
                    .when(!why.is_empty(), |text| {
                        text.child(div().text_xs().text_color(Theme::ink_dim()).child(why))
                    }),
            )
            .child(button("Reject", Verdict::Reject))
            .child(button("Accept", Verdict::Accept))
            .into_any_element()
    }

    fn group(title: &'static str, note: &'static str) -> impl IntoElement {
        div()
            .pt_3()
            .flex()
            .items_baseline()
            .gap_3()
            .child(div().text_color(Theme::ink()).child(title))
            .child(div().text_xs().text_color(Theme::ink_dim()).child(note))
    }

    /// What the tagger proposed and is waiting on a person for.
    pub(super) fn tidy_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match &self.tidy {
            None => div().text_color(Theme::ink_dim()).child("Loading…"),
            Some(tidy) if tidy.waiting() == 0 => div()
                .pt_4()
                .text_color(Theme::ink_dim())
                .child("All tidy. Memory adds things here when it needs you."),
            Some(tidy) => div()
                .flex()
                .flex_col()
                .gap_2()
                .when(!tidy.batches.is_empty(), |list| {
                    list.child(Self::group(
                        "Topics, entities and types",
                        "changes to the trees and the wiki's pages, in the batches they were proposed in",
                    ))
                    .children(self.batches(cx))
                })
                .when(!tidy.tags.is_empty(), |list| {
                    list.child(Self::group("Tags", "the tagger's proposals for conversations"))
                        .children(tidy.tags.iter().enumerate().map(|(index, tag)| {
                            let verb = match tag.change {
                                TagChange::Add => "Add",
                                TagChange::Remove => "Remove",
                            };
                            let what = format!("{verb} {} on {}", tag.tag, tag.conversation);
                            Self::proposal(Item::Tag(index), what, tag.reason.clone(), cx)
                        }))
                })
                .when(!tidy.terms.is_empty(), |list| {
                    list.child(Self::group("Tag values", "new values the tagger wants to use"))
                        .children(tidy.terms.iter().enumerate().map(|(index, term)| {
                            let what = if term.display.is_empty() || term.display == term.value {
                                format!("{}:{}", term.key, term.value)
                            } else {
                                format!("{}:{} ({})", term.key, term.value, term.display)
                            };
                            Self::proposal(Item::Term(index), what, term.reason.clone(), cx)
                        }))
                }),
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_3()
                    .child(div().text_xl().text_color(Theme::ink()).child("To tidy"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child("memory works on its own; these need a person"),
                    ),
            )
            .child(body)
            .into_any_element()
    }
}

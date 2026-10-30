use std::collections::{HashMap, HashSet};

use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::TopicId;
use model::memory::Verdict;
use model::tidy::{Batch, Branch, Change, Proposal, Proposer, Row};
use model::topics::Topic;

use super::{MemoryEvent, MemoryPage};
use crate::brand;
use crate::text::{ago_phrase, now_ms};
use crate::theme::Theme;

/// A batch shows this many of its page changes until asked for all of them.
const SHOWN: usize = 12;
const INDENT: f32 = 22.;
const SAMPLES: usize = 3;

/// How the person has laid out the batches they're reviewing.
#[derive(Debug, Default)]
pub(super) struct Review {
    /// The folded branches, by batch and the topic each heads.
    folded: HashSet<(String, TopicId)>,
    /// The proposals whose sessions are listed, and whether all of them are.
    pub(super) members: HashMap<String, bool>,
    /// The batches whose page changes are all listed.
    all_pages: HashSet<String>,
}

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

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
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

    /// A wiki page's title where this app has listed it, else its slug.
    fn page_named(&self, slug: &str) -> String {
        self.loose
            .iter()
            .map(|listing| (&listing.slug, &listing.title))
            .chain(
                self.library
                    .iter()
                    .flat_map(|library| library.pages.iter().map(|page| (&page.slug, &page.title))),
            )
            .find(|(known, _)| *known == slug)
            .map_or_else(|| slug.to_owned(), |(_, title)| title.clone())
    }

    /// The change in words, such as "New topic Rail under Desktop App". A change sitting in the
    /// tree under what it changes or where it goes leaves that out.
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
            Change::FileInto {
                topic,
                sessions,
                conversations,
            } => {
                let what = counted(sessions.len() + conversations.len(), "session", "sessions");
                if nested {
                    format!("File {what} here")
                } else {
                    format!("File {what} into {}", name(topic))
                }
            }
            Change::AddType {
                name: new, parent, ..
            } => format!("New type {new}{}", under(parent)),
            Change::MergePage { page, into } => {
                format!(
                    "Merge the page {} into {}",
                    self.page_named(page),
                    self.page_named(into)
                )
            }
            Change::RemovePage { page } => format!("Remove the page {}", self.page_named(page)),
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

    /// The tree changes waiting, each batch as the tree it proposes with a way to decide all of it.
    pub(super) fn batches(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(tidy) = &self.tidy else {
            return Vec::new();
        };
        let existing: Vec<Topic> = self
            .listed
            .topics
            .iter()
            .chain(self.catalog.iter().flat_map(|catalog| catalog.types.iter()))
            .cloned()
            .collect();
        tidy.batches
            .iter()
            .enumerate()
            .map(|(index, batch)| self.batch(index, batch, &existing, cx))
            .collect()
    }

    fn batch(&self, index: usize, batch: &Batch, existing: &[Topic], cx: &mut Context<Self>) -> AnyElement {
        let rows = batch.tree(existing);
        let pages: Vec<&Proposal> = batch.pages().collect();
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
        if !pages.is_empty() {
            summary.push(counted(pages.len(), "page change", "page changes"));
        }
        summary.push(ago_phrase(now_ms(), batch.at_ms));
        let (approve, reject) = (batch.id.clone(), batch.id.clone());
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
            .children(self.tree(batch, &rows, cx))
            .children(self.pages(batch, &pages, cx))
            .into_any_element()
    }

    /// Folds a branch of a batch's tree, or opens it again.
    fn folding(
        key: (String, TopicId),
        cx: &mut Context<Self>,
    ) -> impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static {
        cx.listener(move |this, _: &ClickEvent, _, cx| {
            if !this.review.folded.remove(&key) {
                this.review.folded.insert(key.clone());
            }
            cx.notify();
        })
    }

    /// The proposed tree's rows, leaving out those inside a folded branch.
    fn tree(&self, batch: &Batch, rows: &[Row<'_>], cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::with_capacity(rows.len());
        let mut folded_at: Option<usize> = None;
        for (at, row) in rows.iter().enumerate() {
            if folded_at.is_some_and(|depth| row.depth > depth) {
                continue;
            }
            folded_at = None;
            let opens = rows.get(at + 1).is_some_and(|next| next.depth > row.depth);
            let folded = row
                .heads
                .is_some_and(|topic| self.review.folded.contains(&(batch.id.clone(), topic.clone())));
            if opens && folded {
                folded_at = Some(row.depth);
            }
            let key = row.heads.map(|topic| (batch.id.clone(), topic.clone()));
            let fold = key.clone().filter(|_| opens).map(|key| {
                div()
                    .id(("fold", at))
                    .flex_none()
                    .cursor_pointer()
                    .on_click(Self::folding(key, cx))
                    .child(brand::icon(
                        brand::symbol(Some(if folded { "chevron_right" } else { "expand_more" })),
                        18.,
                        Theme::ink_dim(),
                    ))
            });
            let indent = px(INDENT * f32::from(u8::try_from(row.depth.min(6)).unwrap_or(6)));
            let line = match row.branch {
                // A topic that exists has nothing to decide, so the whole row folds its branch.
                Branch::Topic(topic) => div()
                    .id(("existing", at))
                    .min_h(px(32.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .text_color(Theme::ink())
                    .when_some(key.filter(|_| opens), |line, key| {
                        line.cursor_pointer().on_click(Self::folding(key, cx))
                    })
                    .children(fold)
                    .child(self.named(batch, topic))
                    .into_any_element(),
                Branch::Proposal(proposal) => self.proposed(batch, row.depth > 0, proposal, fold, cx),
            };
            out.push(div().ml(indent).child(line).into_any_element());
            if let Branch::Proposal(proposal) = row.branch
                && self.review.members.contains_key(&proposal.id)
            {
                out.push(
                    div()
                        .ml(indent + px(INDENT))
                        .child(self.members(proposal, cx))
                        .into_any_element(),
                );
            }
        }
        out
    }

    /// The wiki page changes under the tree, the first few until all are asked for.
    fn pages(&self, batch: &Batch, pages: &[&Proposal], cx: &mut Context<Self>) -> Vec<AnyElement> {
        if pages.is_empty() {
            return Vec::new();
        }
        let all = self.review.all_pages.contains(&batch.id);
        let shown = if all { pages.len() } else { pages.len().min(SHOWN) };
        let mut out = vec![
            div()
                .pt_2()
                .px_2()
                .text_color(Theme::ink())
                .child(format!(
                    "Wiki pages · {}",
                    counted(pages.len(), "change", "changes")
                ))
                .into_any_element(),
        ];
        out.extend(
            pages
                .iter()
                .take(shown)
                .map(|proposal| self.proposed(batch, false, proposal, None, cx)),
        );
        if pages.len() > SHOWN {
            let id = batch.id.clone();
            out.push(
                div()
                    .id(("pages-all", batch.proposals.len()))
                    .pt_1()
                    .px_2()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(Theme::accent())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if !this.review.all_pages.remove(&id) {
                            this.review.all_pages.insert(id.clone());
                        }
                        cx.notify();
                    }))
                    .child(if all {
                        "Show fewer".to_owned()
                    } else {
                        format!("Show all {}", pages.len())
                    })
                    .into_any_element(),
            );
        }
        out
    }

    /// How many sessions a proposal would file, opening the list of them.
    fn members_chip(
        &self,
        row: usize,
        proposal: &Proposal,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        let (sessions, conversations) = proposal.change.members()?;
        let count = sessions.len() + conversations.len();
        if count == 0 {
            return None;
        }
        let open = self.review.members.contains_key(&proposal.id);
        let id = proposal.id.clone();
        Some(
            div()
                .id(("members", row))
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .rounded(px(10.))
                .cursor_pointer()
                .text_xs()
                .text_color(Theme::accent())
                .hover(|chip| chip.bg(Theme::raised()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if this.review.members.remove(&id).is_none() {
                        this.review.members.insert(id.clone(), false);
                    }
                    cx.notify();
                }))
                .child(counted(count, "session", "sessions"))
                .child(brand::icon(
                    brand::symbol(Some(if open { "expand_less" } else { "expand_more" })),
                    16.,
                    Theme::accent(),
                )),
        )
    }

    fn proposed(
        &self,
        batch: &Batch,
        nested: bool,
        proposal: &Proposal,
        fold: Option<gpui::Stateful<gpui::Div>>,
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
            .filter_map(|sample| sample.title.as_deref())
            .take(SAMPLES)
            .collect();
        let aliases = match &proposal.change {
            Change::CreateTopic { aliases, .. } if !aliases.is_empty() => {
                Some(format!("Also called {}", aliases.join(", ")))
            }
            _ => None,
        };
        let members = self.members_chip(row, proposal, cx);
        let (approve, reject) = (proposal.id.clone(), proposal.id.clone());
        div()
            .id(("proposed", row))
            .px_4()
            .py_2()
            .rounded(px(14.))
            .bg(Theme::card())
            .flex()
            .items_center()
            .gap_3()
            .children(fold)
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
                        nested,
                    )))
                    .children(
                        aliases.map(|aliases| div().text_xs().text_color(Theme::ink_dim()).child(aliases)),
                    )
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
            .children(members)
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

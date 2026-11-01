use std::collections::{HashMap, HashSet};

use gateway::GatewayError;
use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::TopicId;
use model::memory::Verdict;
use model::tidy::{Batch, Change, Kind, Proposal};
use model::topics::Topic;

use super::{MemoryEvent, MemoryPage};
use crate::brand;
use crate::theme::Theme;

const SAMPLES: usize = 3;

/// How the person has laid out the batches they're reviewing. Each batch starts as its kinds of
/// change with their counts, each kind closed.
#[derive(Debug, Default)]
pub(super) struct Review {
    /// The kinds of change opened, by batch.
    pub(super) opened: HashSet<(String, Kind)>,
    /// The proposals whose sessions are listed, and whether all of them are.
    pub(super) members: HashMap<String, bool>,
    /// The kinds of change listed in full, by batch.
    pub(super) all: HashSet<(String, Kind)>,
    /// A verdict on a whole batch, waiting for the person to confirm it.
    pub(super) confirming: Option<(String, Verdict)>,
}

#[derive(Debug, Clone)]
pub(super) enum Decided {
    One(String),
    Batch(String),
}

pub(super) fn verdict_button(
    id: (&'static str, usize),
    label: impl Into<gpui::SharedString>,
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
        .child(label.into())
}

pub(super) fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// What became of the proposals rh couldn't decide. One no longer pending was out of date, and rh
/// set it aside; the others stay pending, with rh's reason.
fn undecided(failed: &[(String, String)], tidy: Option<&app::Tidy>) -> Option<String> {
    if failed.is_empty() {
        return None;
    }
    let pending = |id: &str| {
        tidy.is_none_or(|tidy| {
            tidy.batches
                .iter()
                .any(|batch| batch.proposals.iter().any(|proposal| proposal.id == id))
        })
    };
    let (stayed, set_aside): (Vec<_>, Vec<_>) = failed.iter().partition(|(id, _)| pending(id));
    let mut said = Vec::new();
    if !set_aside.is_empty() {
        let them = if set_aside.len() == 1 { "it" } else { "them" };
        said.push(format!(
            "{} out of date, so memory set {them} aside.",
            counted(set_aside.len(), "change was", "changes were")
        ));
    }
    if !stayed.is_empty() {
        let reasons: Vec<&str> = stayed.iter().map(|(_, why)| why.as_str()).collect();
        said.push(format!(
            "{} pending: {}",
            counted(stayed.len(), "change stayed", "changes stayed"),
            reasons.join("; ")
        ));
    }
    Some(said.join(" "))
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

    pub(super) fn decide_proposals(&mut self, decided: Decided, verdict: Verdict, cx: &mut Context<Self>) {
        self.problem = None;
        self.review.confirming = None;
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
                Decided::One(id) => match hub.decide_proposal(id.clone(), verdict).await {
                    Ok(()) => Ok(Vec::new()),
                    Err(GatewayError::Conflict(why)) => Ok(vec![(id, why)]),
                    Err(err) => Err(err),
                },
                Decided::Batch(id) => hub.decide_batch(id, verdict).await,
            };
            let tidy = hub.tidy().await;
            this.update(cx, |this, cx| {
                this.problem = match outcome {
                    Ok(failed) => undecided(&failed, tidy.as_ref().ok()),
                    Err(err) => Some(format!("deciding it failed: {err}")),
                };
                match tidy {
                    Ok(tidy) => this.tidy = Some(tidy),
                    Err(err) => this.failed(&err, cx),
                }
                if verdict == Verdict::Accept {
                    cx.emit(MemoryEvent::Refiled);
                    this.load_catalog(cx);
                    // A page merge or removal changes the wiki's pages, which Topics keeps listed.
                    this.load_library(cx);
                    this.load_pages(cx);
                }
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

    pub(super) fn proposed(
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

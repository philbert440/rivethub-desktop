use std::collections::{BTreeSet, HashSet};

use app::TrashReview;
use gpui::{AnyElement, ClickEvent, Context, ParentElement, Styled, div, prelude::*, px};
use model::SessionId;
use model::memory::Verdict;
use model::trash::{Deletion, Trash};

use super::proposals::{counted, verdict_button};
use super::{MemoryEvent, MemoryPage};
use crate::text::{ago_phrase, now_ms};
use crate::theme::Theme;

/// The trash review: what the nodes count as obvious trash, less what the person kept.
#[derive(Debug, Default)]
pub(super) struct Sweep {
    review: Option<TrashReview>,
    /// The nodes are being asked; the first answer can take a while on a big memory.
    loading: bool,
    /// rh keeps no record of a keep, so a kept session stays off the review while the app runs.
    kept: HashSet<SessionId>,
    /// Delete all is asking first.
    confirming: bool,
    problem: Option<String>,
}

impl Sweep {
    pub(super) fn waiting(&self) -> usize {
        self.review.as_ref().map_or(0, |review| review.trash.len())
    }

    fn drop(&mut self, gone: impl Fn(&SessionId) -> bool) {
        if let Some(review) = self.review.as_mut() {
            review.trash.retain(|trash| !gone(&trash.session));
        }
    }
}

/// A Delete button the size of the verdict buttons, in the colours a delete gets.
fn delete_button(id: (&'static str, usize), label: String) -> gpui::Stateful<gpui::Div> {
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
        .bg(Theme::warn_container())
        .text_color(Theme::warn())
        .child(label)
}

impl MemoryPage {
    pub(super) fn load_trash(&mut self, cx: &mut Context<Self>) {
        self.trash.loading = true;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.trash().await;
            this.update(cx, |this, cx| {
                this.trash.loading = false;
                match loaded {
                    Ok(mut review) => {
                        review
                            .trash
                            .retain(|trash| !this.trash.kept.contains(&trash.session));
                        this.trash.review = Some(review);
                    }
                    Err(err) => this.trash.problem = Some(format!("the trash review: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn keep(&mut self, session: &SessionId, cx: &mut Context<Self>) {
        self.trash.kept.insert(session.clone());
        self.trash.drop(|listed| listed == session);
        cx.notify();
    }

    /// Deletes the sessions, which leave the review and the workspace's lists at once; any that
    /// stay come back with why.
    fn sweep(&mut self, doomed: Vec<Trash>, cx: &mut Context<Self>) {
        self.trash.problem = None;
        self.trash.confirming = false;
        let going: HashSet<SessionId> = doomed.iter().map(|trash| trash.session.clone()).collect();
        self.trash.drop(|session| going.contains(session));
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let outcomes = hub.delete_trash(doomed).await;
            this.update(cx, |this, cx| {
                let (deleted, stayed): (Vec<_>, Vec<_>) = outcomes
                    .into_iter()
                    .partition(|(_, outcome)| *outcome == Deletion::Deleted);
                if !deleted.is_empty() {
                    cx.emit(MemoryEvent::SessionsDeleted(
                        deleted.into_iter().map(|(session, _)| session).collect(),
                    ));
                    this.load_stats(cx);
                }
                if !stayed.is_empty() {
                    let why: BTreeSet<&str> = stayed
                        .iter()
                        .filter_map(|(_, outcome)| outcome.refusal())
                        .collect();
                    this.trash.problem = Some(format!(
                        "{} stayed: {}.",
                        counted(stayed.len(), "session", "sessions"),
                        why.into_iter().collect::<Vec<_>>().join("; ")
                    ));
                    this.load_trash(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The node's name, this client's own for `None`.
    fn node_name(&self, node: Option<&String>) -> String {
        node.cloned()
            .or_else(|| self.hub.local_node())
            .unwrap_or_else(|| "this node".to_owned())
    }

    /// Delete all, or while it asks first, Cancel and the delete itself.
    fn trash_actions(&self, count: usize, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if count == 0 {
            return Vec::new();
        }
        if !self.trash.confirming {
            return vec![
                delete_button(("trash-all", 0), "Delete all".to_owned())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.trash.confirming = true;
                        cx.notify();
                    }))
                    .into_any_element(),
            ];
        }
        vec![
            verdict_button(("trash-cancel", 0), "Cancel", Verdict::Reject)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.trash.confirming = false;
                    cx.notify();
                }))
                .into_any_element(),
            delete_button(
                ("trash-confirm", 0),
                format!("Delete {}", counted(count, "session", "sessions")),
            )
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                let doomed = this.trash.review.as_ref().map(|review| review.trash.clone());
                this.sweep(doomed.unwrap_or_default(), cx);
            }))
            .into_any_element(),
        ]
    }

    fn trash_heading(&self, review: &TrashReview, now: i64) -> gpui::Div {
        let heading = div().flex_1().min_w_0();
        if self.trash.confirming {
            return heading.text_color(Theme::ink()).child(
                "Delete all of them? Memory forgets them; their transcripts stay, and a session used again comes back.",
            );
        }
        let mut summary = vec![counted(review.trash.len(), "session", "sessions")];
        if let Some(newest) = review.trash.first() {
            summary.push(format!("newest {}", ago_phrase(now, newest.updated_at_ms)));
        }
        if !review.unasked.is_empty() {
            let unasked: Vec<String> = review
                .unasked
                .iter()
                .map(|node| self.node_name(node.as_ref()))
                .collect();
            summary.push(format!("{} couldn't say", unasked.join(", ")));
        }
        heading
            .flex()
            .flex_col()
            .child(div().text_color(Theme::ink()).child("Obvious trash"))
            .child(
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(summary.join(" · ")),
            )
    }

    /// One session to keep or delete: its title, then its harness, node, last activity and why.
    fn trash_row(&self, index: usize, trash: &Trash, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let facts = [
            trash.session.harness().display_name().to_owned(),
            self.node_name(trash.node.as_ref()),
            ago_phrase(now, trash.updated_at_ms),
            trash.why(),
        ];
        let (kept, doomed) = (trash.session.clone(), trash.clone());
        div()
            .id(("trash", index))
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
                    .child(div().text_color(Theme::ink()).truncate().child(title(trash)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .truncate()
                            .child(facts.join(" · ")),
                    ),
            )
            .child(
                verdict_button(("trash-keep", index), "Keep", Verdict::Reject)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.keep(&kept, cx))),
            )
            .child(
                delete_button(("trash-delete", index), "Delete".to_owned()).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.sweep(vec![doomed.clone()], cx)),
                ),
            )
            .into_any_element()
    }

    /// What the nodes count as obvious trash, as a batch to decide one at a time or all at once.
    pub(super) fn trash_review(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(review) = self.trash.review.as_ref() else {
            let (note, ink) = if self.trash.loading {
                (
                    "Asking each node for its obvious trash…".to_owned(),
                    Theme::ink_dim(),
                )
            } else {
                (self.trash.problem.clone()?, Theme::warn())
            };
            return Some(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(ink)
                    .child(note)
                    .into_any_element(),
            );
        };
        if review.trash.is_empty() && self.trash.problem.is_none() {
            return None;
        }
        let now = now_ms();
        let rows: Vec<AnyElement> = review
            .trash
            .iter()
            .enumerate()
            .map(|(index, trash)| self.trash_row(index, trash, now, cx))
            .collect();
        Some(
            div()
                .id("trash-review")
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
                        .child(self.trash_heading(review, now))
                        .children(self.trash_actions(review.trash.len(), cx)),
                )
                .children(
                    self.trash
                        .problem
                        .clone()
                        .map(|problem| div().px_2().text_xs().text_color(Theme::warn()).child(problem)),
                )
                .children(rows)
                .into_any_element(),
        )
    }
}

/// The session's title, or for one without, its id's start, so untitled rows can be told apart.
fn title(trash: &Trash) -> String {
    if let Some(title) = trash.title.as_deref().filter(|title| !title.trim().is_empty()) {
        return title.to_owned();
    }
    let native = trash.session.native().as_str();
    format!("Untitled · {}", native.get(..8).unwrap_or(native))
}
